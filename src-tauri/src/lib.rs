mod beanfun;
mod browser;

use beanfun::{GameAccount, QrInit, QrPollOutcome};
use reqwest_cookie_store::CookieStoreMutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

struct QrSession {
    client: reqwest::Client,
    cookie_store: Arc<CookieStoreMutex>,
    init: QrInit,
}

struct AppState {
    pending_qr: Mutex<Option<QrSession>>,
    /// token → cookie_store for active sessions (OTP reuses the login cookie jar)
    session_stores: Mutex<HashMap<String, Arc<CookieStoreMutex>>>,
}

fn map_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ─── QR Start ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct QrStartResult {
    bitmap_base64: String,
    deeplink: Option<String>,
}

#[tauri::command]
async fn qr_start(state: tauri::State<'_, AppState>) -> Result<QrStartResult, String> {
    let (client, cookie_store) = beanfun::build_client_with_store().map_err(map_err)?;
    let skey = beanfun::get_session_key(&client).await.map_err(map_err)?;
    let init = beanfun::init_qr_login(&client, &skey).await.map_err(map_err)?;

    let result = QrStartResult {
        bitmap_base64: init.bitmap_base64.clone(),
        deeplink: init.deeplink.clone(),
    };

    *state.pending_qr.lock().await = Some(QrSession { client, cookie_store, init });
    Ok(result)
}

// ─── QR Check ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum QrCheckResult {
    Waiting,
    Expired,
    Approved { token: String, games: Vec<GameAccount> },
}

#[tauri::command]
async fn qr_check(state: tauri::State<'_, AppState>) -> Result<QrCheckResult, String> {
    let (client, cookie_store, init) = {
        let guard = state.pending_qr.lock().await;
        match guard.as_ref() {
            None => return Err("No active QR session. Call qr_start first.".into()),
            Some(s) => (s.client.clone(), s.cookie_store.clone(), s.init.clone()),
        }
    };

    let outcome = beanfun::poll_qr(&client, &init).await.map_err(map_err)?;

    match outcome {
        QrPollOutcome::Waiting => Ok(QrCheckResult::Waiting),
        QrPollOutcome::Expired => {
            *state.pending_qr.lock().await = None;
            Ok(QrCheckResult::Expired)
        }
        QrPollOutcome::Approved => {
            let token = beanfun::finalize_qr(&client, &cookie_store, &init)
                .await
                .map_err(map_err)?;
            let games = beanfun::get_game_accounts(&client, &token).await.unwrap_or_default();
            state.session_stores.lock().await
                .insert(token.clone(), cookie_store);
            *state.pending_qr.lock().await = None;
            Ok(QrCheckResult::Approved { token, games })
        }
    }
}

// ─── Windows helpers ─────────────────────────────────────────────────────────

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, MAPVK_VK_TO_VSC,
        SendInput, INPUT, INPUT_0,
        INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        INPUT_MOUSE, MOUSEINPUT,
        MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
        VK_BACK, VK_END, VK_RETURN, VK_TAB,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows,
        GetClientRect, GetSystemMetrics, GetWindowRect, GetWindowTextW,
        IsWindowVisible, PostMessageW,
        SetForegroundWindow, ShowWindow, SW_RESTORE,
        SM_CXSCREEN, SM_CYSCREEN,
        SystemParametersInfoW, SPI_GETWORKAREA,
        WM_KEYDOWN, WM_KEYUP,
    };

    /// Primary monitor **work area** (screen minus taskbar) as screen-coordinate
    /// `(left, top, right, bottom)`. Uses `SPI_GETWORKAREA` rather than the raw
    /// screen size so a bottom-right placed window isn't clipped by the taskbar.
    pub fn primary_work_area() -> Option<(i32, i32, i32, i32)> {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                &mut r as *mut RECT as *mut core::ffi::c_void,
                0,
            )
        };
        if ok == 0 { return None; }
        Some((r.left, r.top, r.right, r.bottom))
    }

    unsafe fn post_key(hwnd: HWND, vk: u32) {
        let scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
        let dn = ((scan << 16) | 1) as isize;
        let up = ((scan << 16) | 0xC000_0001u32) as isize;
        PostMessageW(hwnd, WM_KEYDOWN, vk as usize, dn);
        PostMessageW(hwnd, WM_KEYUP, vk as usize, up);
    }

    unsafe fn send_mouse_click(screen_x: i32, screen_y: i32) {
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let nx = ((screen_x as i64 * 65535) / sw as i64) as i32;
        let ny = ((screen_y as i64 * 65535) / sh as i64) as i32;
        let mi = |flags: u32| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: nx, dy: ny, mouseData: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        };
        SendInput(1, [mi(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE)].as_ptr(), std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(60));
        let clicks = [mi(MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_ABSOLUTE), mi(MOUSEEVENTF_LEFTUP | MOUSEEVENTF_ABSOLUTE)];
        SendInput(2, clicks.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }

    unsafe fn send_vk(vk: u16) {
        let inputs = [
            INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: 0, time: 0, dwExtraInfo: 0 } } },
            INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 } } },
        ];
        SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }

    unsafe fn send_char_u16(ch: u16) {
        let inputs = [
            INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: ch, dwFlags: KEYEVENTF_UNICODE, time: 0, dwExtraInfo: 0 } } },
            INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: ch, dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 } } },
        ];
        SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }

    /// Exact window title of the game client. Only a window whose title matches
    /// this precisely counts as "the game" — deliberately strict so an unrelated
    /// window (a browser tab, a chat) is never mistaken for it.
    const GAME_WINDOW_TITLE: &str = "MapleStory";

    unsafe extern "system" fn game_title_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 { return 1; }
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            if title.trim() == GAME_WINDOW_TITLE {
                *(lparam as *mut HWND) = hwnd;
                return 0;
            }
        }
        1
    }

    /// Locate the running game window by its exact title. Null when not open.
    unsafe fn find_game_window() -> HWND {
        let mut hwnd: HWND = std::ptr::null_mut();
        EnumWindows(Some(game_title_cb), &mut hwnd as *mut HWND as LPARAM);
        hwnd
    }

    /// True when a MapleStory client window is currently open.
    pub fn is_game_running() -> bool {
        unsafe { !find_game_window().is_null() }
    }

    /// Hand a target (file path or protocol URI such as `gamaniagames://…`) to
    /// its registered handler via ShellExecute. Used to launch the game through
    /// the local Gamania Games Manager. (Explorer can't resolve custom schemes,
    /// so it must be ShellExecute, not `explorer.exe <uri>`.)
    ///
    /// Note: under `tauri dev` the launched game shares our job object and dies
    /// when the app restarts on a rebuild — a dev-only artifact; a packaged build
    /// has no such job, so the game keeps running independently.
    pub fn shell_open(target: &str) -> Result<(), String> {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let target_wide: Vec<u16> = target.encode_utf16().chain(Some(0u16)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0u16)).collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target_wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if (result as usize) <= 32 {
            return Err(format!("無法啟動：錯誤碼 {}", result as usize));
        }
        Ok(())
    }

    /// Type the account id + OTP into the running MapleStory login form.
    pub fn fill_login_form(account_id: &str, otp: &str) -> Result<(), String> {
        let hwnd = unsafe { find_game_window() };
        if hwnd.is_null() {
            return Err("找不到遊戲視窗，請先開啟楓之谷".to_string());
        }

        unsafe { ShowWindow(hwnd, SW_RESTORE); SetForegroundWindow(hwnd); }
        std::thread::sleep(std::time::Duration::from_millis(400));

        let mut win_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut cli_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe {
            GetWindowRect(hwnd, &mut win_rect);
            GetClientRect(hwnd, &mut cli_rect);
        }
        let win_w = win_rect.right - win_rect.left;
        let win_h = win_rect.bottom - win_rect.top;
        let nc_x = (win_w - cli_rect.right) / 2;
        let nc_y = win_h - cli_rect.bottom - nc_x;
        let cli_sx = win_rect.left + nc_x;
        let cli_sy = win_rect.top + nc_y;

        // Click the account field (~50%, 40%) via SendInput to bypass UIPI.
        let mid_x = cli_sx + cli_rect.right / 2;
        let acc_y = cli_sy + cli_rect.bottom * 40 / 100;
        unsafe { send_mouse_click(mid_x, acc_y) };
        std::thread::sleep(std::time::Duration::from_millis(350));

        unsafe {
            post_key(hwnd, VK_END as u32);
            for _ in 0..64 { post_key(hwnd, VK_BACK as u32); }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        unsafe { for ch in account_id.encode_utf16() { send_char_u16(ch); } }
        std::thread::sleep(std::time::Duration::from_millis(150));

        unsafe { send_vk(VK_TAB as u16); }
        std::thread::sleep(std::time::Duration::from_millis(150));

        unsafe { for ch in otp.encode_utf16() { send_char_u16(ch); } }
        std::thread::sleep(std::time::Duration::from_millis(150));

        unsafe { send_vk(VK_RETURN as u16); }
        Ok(())
    }
}


// ─── Smart arrow: launch if closed, fill login if already open ─────────────────

/// The per-account arrow action. If MapleStory is already running, fetch the OTP
/// and type account+OTP into its login form; otherwise launch the game via GGM.
/// Returns `"filled"` or `"launched"` so the UI can report what happened.
#[tauri::command]
async fn smart_launch(
    state: tauri::State<'_, AppState>,
    token: String,
    account_sn: String,
    account_sid: String,
    account_sname: String,
) -> Result<String, String> {
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        stores.get(&token).cloned()
            .ok_or_else(|| "SESSION_EXPIRED".to_string())?
    };

    #[cfg(windows)]
    let running = win::is_game_running();
    #[cfg(not(windows))]
    let running = false;

    if running {
        let result = beanfun::get_otp(&cookie_store, &token, &account_sn, &account_sid, &account_sname)
            .await
            .map_err(map_err)?;
        #[cfg(windows)]
        {
            let sid = result.sid.clone();
            let otp = result.otp.clone();
            tokio::task::spawn_blocking(move || win::fill_login_form(&sid, &otp))
                .await
                .map_err(|e| e.to_string())??;
        }
        Ok("filled".to_string())
    } else {
        let uri = beanfun::build_launch_uri(&cookie_store, &token, &account_sn)
            .await
            .map_err(map_err)?;
        #[cfg(windows)]
        {
            tokio::task::spawn_blocking(move || win::shell_open(&uri))
                .await
                .map_err(|e| e.to_string())??;
        }
        #[cfg(not(windows))]
        {
            let _ = uri;
        }
        Ok("launched".to_string())
    }
}

// ─── Launch via GGM (one-click) ────────────────────────────────────────────────

/// Fetch the game_start_step2 launch blob for this account and hand it to the
/// local Gamania Games Manager via the `gamaniagames://` protocol. Replaces the
/// old "type the OTP into the game" flow, which beanfun retired on 2026-08-17.
#[tauri::command]
async fn launch_via_ggm(
    state: tauri::State<'_, AppState>,
    token: String,
    account_sn: String,
) -> Result<(), String> {
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        stores.get(&token).cloned()
            .ok_or_else(|| "SESSION_EXPIRED".to_string())?
    };

    let uri = beanfun::build_launch_uri(&cookie_store, &token, &account_sn)
        .await
        .map_err(map_err)?;

    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(move || win::shell_open(&uri))
            .await
            .map_err(|e| e.to_string())??;
    }
    #[cfg(not(windows))]
    {
        let _ = uri;
    }
    Ok(())
}

// ─── OTP (revived with GGM integrity params) ───────────────────────────────────

#[derive(Serialize)]
struct OtpResponse {
    sid: String,
    otp: String,
}

/// Fetch the game login OTP for an account. Revived on 2026-08-18 after the
/// endpoint's `Query String Error` turned out to be a missing CV/Hash/arch
/// integrity trio, not a dead endpoint. Returns the short numeric OTP the game
/// login box expects (alongside the ServiceAccount).
#[tauri::command]
async fn get_otp(
    state: tauri::State<'_, AppState>,
    token: String,
    account_sn: String,
    account_sid: String,
    account_sname: String,
) -> Result<OtpResponse, String> {
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        stores.get(&token).cloned()
            .ok_or_else(|| "SESSION_EXPIRED".to_string())?
    };
    let result = beanfun::get_otp(&cookie_store, &token, &account_sn, &account_sid, &account_sname)
        .await
        .map_err(map_err)?;
    Ok(OtpResponse { sid: result.sid, otp: result.otp })
}

// ─── Share Launch URI (sender side) ────────────────────────────────────────────

/// Build the `gamaniagames://` launch URI for an account and return it (without
/// launching). The owner copies this and shares it (e.g. via Discord); the
/// recipient opens it with 代理登入. Same blob GGM consumes locally — no OTP.
#[tauri::command]
async fn get_launch_uri(
    state: tauri::State<'_, AppState>,
    token: String,
    account_sn: String,
) -> Result<String, String> {
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        stores.get(&token).cloned()
            .ok_or_else(|| "SESSION_EXPIRED".to_string())?
    };
    beanfun::build_launch_uri(&cookie_store, &token, &account_sn)
        .await
        .map_err(map_err)
}

// ─── Proxy Launch (open a shared gamaniagames:// URI) ──────────────────────────

/// Consume a `gamaniagames://` login someone shared (owner posts it → recipient
/// copies it → clicks 代理登入). Smart, like the per-account arrow: if the game
/// is already open, decrypt the shared blob for its OTP and type it into the
/// login form; otherwise launch the game via GGM. The scheme is validated so this
/// can never be coerced into opening arbitrary clipboard content.
#[tauri::command]
async fn proxy_launch(uri: String) -> Result<String, String> {
    let uri = uri.trim().to_string();
    if !uri.starts_with("gamaniagames://") {
        return Err("剪貼簿內容不是有效的登入金鑰".to_string());
    }

    #[cfg(windows)]
    let running = win::is_game_running();
    #[cfg(not(windows))]
    let running = false;

    if running {
        let (sid, otp) = beanfun::otp_from_uri(&uri).await.map_err(map_err)?;
        #[cfg(windows)]
        {
            tokio::task::spawn_blocking(move || win::fill_login_form(&sid, &otp))
                .await
                .map_err(|e| e.to_string())??;
        }
        Ok("filled".to_string())
    } else {
        #[cfg(windows)]
        {
            let u = uri.clone();
            tokio::task::spawn_blocking(move || win::shell_open(&u))
                .await
                .map_err(|e| e.to_string())??;
        }
        Ok("launched".to_string())
    }
}

// ─── Open External URL ─────────────────────────────────────────────────────────

/// Open an http(s) URL in the user's default browser. Scheme-restricted so it
/// can only ever open a web page, never a local file or arbitrary protocol.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("僅允許 http(s) 連結".to_string());
    }
    #[cfg(windows)]
    {
        win::shell_open(url)?;
    }
    #[cfg(not(windows))]
    {
        let _ = url;
    }
    Ok(())
}

// ─── GGM update check ──────────────────────────────────────────────────────────

/// Check whether the locally installed GGM is behind the server's latest build.
/// Called at startup — an out-of-date GGM makes the OTP integrity check fail.
#[tauri::command]
async fn check_ggm_update() -> Result<beanfun::GgmUpdate, String> {
    beanfun::check_ggm_update().await.map_err(map_err)
}

/// Download the GGM installer and run it (the installer's own UI then guides the
/// user). Our process is elevated, so the installer launches with admin rights.
#[tauri::command]
async fn update_ggm(url: String) -> Result<(), String> {
    let installer = beanfun::download_installer(&url, "GGMSetup.exe").await.map_err(map_err)?;
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(move || win::shell_open(&installer))
            .await
            .map_err(|e| e.to_string())??;
    }
    #[cfg(not(windows))]
    {
        let _ = installer;
    }
    Ok(())
}

// ─── Game path override (registry) ─────────────────────────────────────────────

/// Read the MapleStory install directory GGM resolves from
/// `HKLM\SOFTWARE\GAMANIA\MapleStory\Path`. Empty if unset.
#[tauri::command]
fn get_game_path() -> String {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_LOCAL_MACHINE;
        RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\GAMANIA\MapleStory")
            .ok()
            .and_then(|k| k.get_value::<String, _>("Path").ok())
            .unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        String::new()
    }
}

/// Point GGM at a manually-chosen MapleStory location by writing
/// `HKLM\SOFTWARE\GAMANIA\MapleStory\Path`. HKLM requires admin — the app is
/// elevated. Accepts either the folder or `MapleStory.exe` (its folder is used).
#[tauri::command]
fn set_game_path(path: String) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("路徑為空".to_string());
    }
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_LOCAL_MACHINE;

        let p = std::path::Path::new(path);
        let dir = if p.extension().map_or(false, |e| e.eq_ignore_ascii_case("exe")) {
            p.parent().map(|x| x.to_path_buf()).unwrap_or_else(|| p.to_path_buf())
        } else {
            p.to_path_buf()
        };
        let dir_str = dir.to_string_lossy().to_string();

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let (key, _) = hklm
            .create_subkey(r"SOFTWARE\GAMANIA\MapleStory")
            .map_err(|e| format!("寫入登錄檔失敗（需要管理員權限）：{e}"))?;
        key.set_value("Path", &dir_str)
            .map_err(|e| format!("寫入 Path 失敗：{e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}

// ─── Account browser ───────────────────────────────────────────────────────────

/// Open the built-in browser carrying this account's login session. `async` on
/// purpose: creating windows from a synchronous command deadlocks on WebView2.
/// Errors the UI acts on: `SESSION_EXPIRED`, `BROWSER_STILL_OPEN`.
#[tauri::command]
async fn open_account_browser(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    token: String,
    account_id: String,
    alias: String,
) -> Result<(), String> {
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        stores.get(&token).cloned()
            .ok_or_else(|| "SESSION_EXPIRED".to_string())?
    };
    browser::open(&app, &account_id, &alias, &cookie_store)
}

/// The browser shell's navigation bar: back / forward / reload / go to a typed URL.
#[tauri::command]
async fn browser_navigate(
    app: tauri::AppHandle,
    action: String,
    url: Option<String>,
) -> Result<(), String> {
    browser::navigate(&app, &action, url.as_deref())
}

/// The browser shell's tab strip: new / activate / close.
#[tauri::command]
async fn browser_tab(
    app: tauri::AppHandle,
    action: String,
    id: Option<u64>,
) -> Result<(), String> {
    browser::tab_command(&app, &action, id)
}

// ─── App self-update ──────────────────────────────────────────────────────────

/// Check GitHub for a newer app release. Current version comes from Tauri's
/// package info (i.e. `tauri.conf.json`), the authoritative version.
#[tauri::command]
async fn check_app_update(app: tauri::AppHandle) -> Result<beanfun::AppUpdate, String> {
    let current = app.package_info().version.to_string();
    beanfun::check_app_update(&current).await.map_err(map_err)
}

/// Download the app installer and launch it. Our process is elevated, so the
/// installer runs with admin rights; the NSIS installer closes the running app.
#[tauri::command]
async fn update_app(url: String) -> Result<(), String> {
    let installer = beanfun::download_installer(&url, "KuZe-Login-setup.exe").await.map_err(map_err)?;
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(move || win::shell_open(&installer))
            .await
            .map_err(|e| e.to_string())??;
    }
    #[cfg(not(windows))]
    {
        let _ = installer;
    }
    Ok(())
}

// ─── Session Ping ─────────────────────────────────────────────────────────────

#[tauri::command]
async fn ping_session(
    state: tauri::State<'_, AppState>,
    token: String,
) -> Result<bool, String> {
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        match stores.get(&token).cloned() {
            Some(s) => s,
            None => return Ok(false),
        }
    };
    match beanfun::check_session_alive(&cookie_store, &token).await {
        Ok(alive) => Ok(alive),
        Err(_) => Ok(false),
    }
}

// ─── App Entry ────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            pending_qr: Mutex::new(None),
            session_stores: Mutex::new(HashMap::new()),
        })
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            qr_start, qr_check, get_otp,
            smart_launch, launch_via_ggm, get_launch_uri, proxy_launch, open_url,
            check_ggm_update, update_ggm, get_game_path, set_game_path, ping_session,
            open_account_browser, browser_navigate, browser_tab,
            check_app_update, update_app
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            app.get_webview_window("main").unwrap().open_devtools();
            // 開場固定在主螢幕工作區右下角：每次啟動都回這個位置、不記憶拖動後的座標。
            // 用工作區（扣掉工作列）而非螢幕尺寸，否則會被工作列蓋掉一截；用 outer_size
            // （含外框）不是設定檔尺寸，DPI 縮放時才不會少算。
            if let Some(w) = app.get_webview_window("main") {
                if let (Some((_, _, right, bottom)), Ok(sz)) = (win::primary_work_area(), w.outer_size()) {
                    let x = right - sz.width as i32;
                    let y = bottom - sz.height as i32;
                    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

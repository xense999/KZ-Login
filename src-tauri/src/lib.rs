mod beanfun;

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

// ─── OTP ──────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OtpResponse {
    sid: String,
    otp: String,
}

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

// ─── Windows helpers ─────────────────────────────────────────────────────────

#[cfg(windows)]
mod win {
    use std::ptr::null;
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
        EnumChildWindows, EnumWindows, FindWindowW,
        GetClassNameW, GetClientRect, GetSystemMetrics, GetWindowRect, GetWindowTextW,
        IsWindowVisible, PostMessageW,
        SetForegroundWindow, ShowWindow, SW_RESTORE,
        SM_CXSCREEN, SM_CYSCREEN,
        WM_KEYDOWN, WM_KEYUP,
    };

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
        // Move first, then hover, then click
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

    unsafe extern "system" fn edit_collector(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 { return 1; }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), 64);
        if len > 0 {
            let cls = String::from_utf16_lossy(&buf[..len as usize]);
            let is_edit = cls.eq_ignore_ascii_case("Edit")
                || cls.eq_ignore_ascii_case("RichEdit20W")
                || cls.eq_ignore_ascii_case("RICHEDIT60W")
                || cls.eq_ignore_ascii_case("RichEdit");
            if is_edit {
                let v = &mut *(lparam as *mut Vec<HWND>);
                v.push(hwnd);
            }
        }
        1
    }

    unsafe extern "system" fn game_title_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 { return 1; }
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            if title.contains("maplestory") || title.contains("楓之谷") || title.contains("maple story") {
                *(lparam as *mut HWND) = hwnd;
                return 0;
            }
        }
        1
    }

    #[allow(dead_code)]
    struct WinInfo { hwnd: HWND, title: String, edit_count: usize }

    unsafe extern "system" fn scan_all_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 { return 1; }
        let mut tbuf = [0u16; 256];
        let tlen = GetWindowTextW(hwnd, tbuf.as_mut_ptr(), 256);
        let title = if tlen > 0 {
            String::from_utf16_lossy(&tbuf[..tlen as usize])
        } else { String::new() };
        let mut edits: Vec<HWND> = Vec::new();
        EnumChildWindows(hwnd, Some(edit_collector), &mut edits as *mut Vec<HWND> as LPARAM);
        let wins = &mut *(lparam as *mut Vec<WinInfo>);
        wins.push(WinInfo { hwnd, title, edit_count: edits.len() });
        1
    }

    pub fn scan_windows() -> Vec<(String, usize)> {
        let mut wins: Vec<WinInfo> = Vec::new();
        unsafe { EnumWindows(Some(scan_all_cb), &mut wins as *mut Vec<WinInfo> as LPARAM) };
        wins.into_iter()
            .filter(|w| w.edit_count > 0 || !w.title.is_empty())
            .map(|w| (w.title, w.edit_count))
            .collect()
    }

    pub fn fill_login_form(account_id: &str, otp: &str) -> Result<(), String> {
        // Find by class name (DirectX game — no Win32 child edits)
        let class_main: Vec<u16> = "MapleStoryClass\0".encode_utf16().collect();
        let class_tw:   Vec<u16> = "MapleStoryClassTW\0".encode_utf16().collect();

        let mut hwnd = unsafe {
            let h = FindWindowW(class_main.as_ptr(), null());
            if h.is_null() { FindWindowW(class_tw.as_ptr(), null()) } else { h }
        };
        // Fallback: search by window title
        if hwnd.is_null() {
            unsafe { EnumWindows(Some(game_title_cb), &mut hwnd as *mut HWND as LPARAM) };
        }
        if hwnd.is_null() {
            return Err("找不到遊戲視窗，請先開啟楓之谷".to_string());
        }

        unsafe { ShowWindow(hwnd, SW_RESTORE); SetForegroundWindow(hwnd); }
        std::thread::sleep(std::time::Duration::from_millis(400));

        // Compute screen coordinates from window rect + client rect
        let mut win_rect  = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut cli_rect  = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe {
            GetWindowRect(hwnd, &mut win_rect);
            GetClientRect(hwnd, &mut cli_rect);
        }
        let win_w = win_rect.right - win_rect.left;
        let win_h = win_rect.bottom - win_rect.top;
        // NC offsets (title bar + borders)
        let nc_x = (win_w - cli_rect.right) / 2;
        let nc_y = win_h - cli_rect.bottom - nc_x;
        let cli_sx = win_rect.left + nc_x;          // client left in screen coords
        let cli_sy = win_rect.top  + nc_y;           // client top in screen coords
        let mid_x  = cli_sx + cli_rect.right / 2;

        // SendInput mouse click on account field (50%, 40%) — bypasses UIPI
        let acc_y = cli_sy + cli_rect.bottom * 40 / 100;
        unsafe { send_mouse_click(mid_x, acc_y) };
        std::thread::sleep(std::time::Duration::from_millis(350));

        // Clear + fill account field
        unsafe {
            post_key(hwnd, VK_END as u32);
            for _ in 0..64 { post_key(hwnd, VK_BACK as u32); }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        unsafe {
            for ch in account_id.encode_utf16() { send_char_u16(ch); }
        }
        std::thread::sleep(std::time::Duration::from_millis(150));

        // Tab to password field (no second mouse click — only the account field is located)
        unsafe { send_vk(VK_TAB as u16); }
        std::thread::sleep(std::time::Duration::from_millis(150));

        // Fill OTP
        unsafe {
            for ch in otp.encode_utf16() { send_char_u16(ch); }
        }
        std::thread::sleep(std::time::Duration::from_millis(150));

        unsafe { send_vk(VK_RETURN as u16); }
        Ok(())
    }
}

// ─── Auto Login ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct AutoLoginResult {
    sid: String,
    otp: String,
}

#[tauri::command]
async fn auto_login(
    state: tauri::State<'_, AppState>,
    token: String,
    account_sn: String,
    account_sid: String,
    account_sname: String,
) -> Result<AutoLoginResult, String> {
    // 1. Fetch OTP (async)
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        stores.get(&token).cloned()
            .ok_or_else(|| "SESSION_EXPIRED".to_string())?
    };
    let result = beanfun::get_otp(
        &cookie_store, &token, &account_sn, &account_sid, &account_sname,
    )
    .await
    .map_err(map_err)?;

    // 2. All Win32 + input simulation in a blocking thread so the async runtime
    //    cannot regrab focus during sleep.
    let sid = result.sid.clone();
    let otp = result.otp.clone();
    tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        win::fill_login_form(&sid, &otp)?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(AutoLoginResult { sid: result.sid, otp: result.otp })
}

// ─── Top-up Window ────────────────────────────────────────────────────────────

#[tauri::command]
fn open_topup(app: tauri::AppHandle, label: String, url: String, title: String) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.close();
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    let escaped = url.replace('\\', "\\\\").replace('\'', "\\'");
    let init_script = format!(
        r#"(function(){{
            var _t='{}';
            var n=parseInt(sessionStorage.getItem('_bf_n')||'0')+1;
            sessionStorage.setItem('_bf_n',n);
            if(n>=2){{
                sessionStorage.removeItem('_bf_n');
                location.replace(_t);
            }}
        }})();"#,
        escaped
    );

    let parsed_url: tauri::utils::config::WebviewUrl = tauri::WebviewUrl::External(
        url.parse::<reqwest::Url>().map_err(|e| e.to_string())?.into()
    );

    tauri::WebviewWindowBuilder::new(&app, &label, parsed_url)
        .title(&title)
        .inner_size(870.0, 512.0)
        .resizable(true)
        .center()
        .initialization_script(&init_script)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ─── Launch Game ──────────────────────────────────────────────────────────────

#[tauri::command]
fn launch_game(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let exe_path = std::path::Path::new(&path);
        let dir = exe_path.parent().unwrap_or(std::path::Path::new("."));
        let dir_str = dir.to_string_lossy().to_string();

        let path_wide: Vec<u16> = path.encode_utf16().chain(Some(0u16)).collect();
        let dir_wide: Vec<u16> = dir_str.encode_utf16().chain(Some(0u16)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0u16)).collect();

        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                path_wide.as_ptr(),
                std::ptr::null(),
                dir_wide.as_ptr(),
                SW_SHOWNORMAL,
            )
        };

        if (result as usize) <= 32 {
            return Err(format!("無法啟動遊戲：錯誤碼 {:?}（可能需要管理員權限）", result));
        }
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let exe_path = std::path::Path::new(&path);
        let dir = exe_path.parent().unwrap_or(std::path::Path::new("."));
        std::process::Command::new(&path)
            .current_dir(dir)
            .spawn()
            .map_err(|e| format!("無法啟動遊戲：{e}"))?;
        Ok(())
    }
}

// ─── Diagnose Windows ─────────────────────────────────────────────────────────

#[tauri::command]
fn diagnose_windows() -> String {
    #[cfg(windows)]
    {
        let wins = win::scan_windows();
        let lines: Vec<String> = wins.iter()
            .map(|(title, count)| format!("[{}個輸入框] \"{}\"", count, title))
            .collect();
        if lines.is_empty() {
            "找不到任何視窗".to_string()
        } else {
            lines.join("\n")
        }
    }
    #[cfg(not(windows))]
    "非 Windows 系統".to_string()
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
            auto_login, ping_session, diagnose_windows, launch_game, open_topup
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            app.get_webview_window("main").unwrap().open_devtools();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

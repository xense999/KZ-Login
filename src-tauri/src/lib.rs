mod beanfun;
mod browser;
mod captcha;
mod credentials;
mod hidden;
mod icon;
mod keyhook;

use beanfun::{GameAccount, LoginPage, LoginStep, QrInit, QrPollOutcome, SessionState};
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

/// Which request a password login is waiting to (re)send.
#[derive(Clone, Copy)]
enum PasswordStage {
    AccountType,
    AccountLogin,
}

/// A password login paused on a reCAPTCHA demand. Only a successful login hands
/// the password on, to `credentials`, which stores it encrypted.
struct PasswordSession {
    client: reqwest::Client,
    cookie_store: Arc<CookieStoreMutex>,
    page: LoginPage,
    account: String,
    password: String,
    stage: PasswordStage,
}

struct AppState {
    pending_qr: Mutex<Option<QrSession>>,
    pending_password: Mutex<Option<PasswordSession>>,
    /// token → cookie_store for active sessions (OTP reuses the login cookie jar)
    session_stores: Mutex<HashMap<String, Arc<CookieStoreMutex>>>,
}

fn map_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

const NO_PENDING_PASSWORD_LOGIN: &str = "沒有進行中的帳密登入，請重新登入";

// ─── QR Start ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct QrStartResult {
    bitmap_base64: String,
    deeplink: Option<String>,
}

#[tauri::command]
async fn qr_start(state: tauri::State<'_, AppState>) -> Result<QrStartResult, String> {
    // Switching to QR abandons any paused password login, and its password.
    *state.pending_password.lock().await = None;
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
                .insert(token.clone(), cookie_store.clone());
            *state.pending_qr.lock().await = None;
            Ok(QrCheckResult::Approved { token, games })
        }
    }
}

// ─── Password Login ───────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PasswordLoginResult {
    Approved { token: String, games: Vec<GameAccount> },
    Captcha,
    Rejected { message: String },
    UseQr { message: String },
}

#[tauri::command]
async fn password_login_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    account: String,
    password: String,
) -> Result<PasswordLoginResult, String> {
    *state.pending_password.lock().await = None;
    let (client, cookie_store) = beanfun::build_client_with_store().map_err(map_err)?;
    let skey = beanfun::get_session_key(&client).await.map_err(map_err)?;
    let page = beanfun::open_login_page(&client, &skey).await.map_err(map_err)?;
    let session = PasswordSession {
        client,
        cookie_store,
        page,
        account: account.trim().to_owned(),
        password,
        stage: PasswordStage::AccountType,
    };
    run_password_login(&app, &state, session, "").await
}

#[tauri::command]
async fn password_login_resume<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    captcha: String,
) -> Result<PasswordLoginResult, String> {
    let session = state.pending_password.lock().await.take()
        .ok_or(NO_PENDING_PASSWORD_LOGIN)?;
    run_password_login(&app, &state, session, &captcha).await
}

/// The remembered logins, passwords included: the form fills them in.
#[tauri::command]
fn saved_logins<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<Vec<credentials::SavedLogin>, String> {
    credentials::list(&app)
}

#[tauri::command]
fn forget_saved_login<R: tauri::Runtime>(app: tauri::AppHandle<R>, account: String) -> Result<(), String> {
    credentials::forget(&app, &account)
}

#[tauri::command]
fn reorder_saved_logins<R: tauri::Runtime>(app: tauri::AppHandle<R>, accounts: Vec<String>) -> Result<(), String> {
    credentials::reorder(&app, &accounts)
}

/// Opens the checkbox for the paused login. `None` = the user gave up.
#[tauri::command]
async fn captcha_solve<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    palette: captcha::Palette,
    region: captcha::Region,
) -> Result<Option<String>, String> {
    let (page_url, site_key) = {
        let guard = state.pending_password.lock().await;
        let session = guard.as_ref().ok_or(NO_PENDING_PASSWORD_LOGIN)?;
        (session.page.url(), session.page.captcha_site_key.clone())
    };
    if site_key.is_empty() {
        return Err("beanfun 沒有提供驗證金鑰，請改用 QR 登入".into());
    }
    let token = captcha::solve(&app, &page_url, &site_key, &palette, region).await?;
    // Cancelled or timed out: nothing will resume this login, so do not keep
    // its password around until the next one starts.
    if token.is_none() {
        *state.pending_password.lock().await = None;
    }
    Ok(token)
}

/// The login page's cancel button while the checkbox is up.
#[tauri::command]
fn captcha_cancel<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    captcha::cancel(&app);
}

/// Walk the steps from wherever `session` stopped. A captcha demand parks the
/// session so the same step can be resent with a token; any other end drops it.
async fn run_password_login<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    mut session: PasswordSession,
    captcha: &str,
) -> Result<PasswordLoginResult, String> {
    // A token answers exactly one request; the next step needs a fresh one.
    let mut captcha = captcha;
    loop {
        let step = match session.stage {
            PasswordStage::AccountType => {
                beanfun::check_account_type(&session.client, &session.page, &session.account, captcha).await
            }
            PasswordStage::AccountLogin => {
                beanfun::account_login(&session.client, &session.page, &session.account, &session.password, captcha).await
            }
        }
        .map_err(map_err)?;
        captcha = "";

        match (step, session.stage) {
            (LoginStep::Proceed, PasswordStage::AccountType) => session.stage = PasswordStage::AccountLogin,
            (LoginStep::Proceed, PasswordStage::AccountLogin) => break,
            (LoginStep::CaptchaRequired, _) => {
                *state.pending_password.lock().await = Some(session);
                return Ok(PasswordLoginResult::Captcha);
            }
            (LoginStep::Rejected(message), _) => return Ok(PasswordLoginResult::Rejected { message }),
            (LoginStep::UseQr(message), _) => return Ok(PasswordLoginResult::UseQr { message }),
        }
    }

    let token = beanfun::complete_login(&session.client, &session.cookie_store, &session.page.skey)
        .await
        .map_err(map_err)?;
    let games = beanfun::get_game_accounts(&session.client, &token).await.unwrap_or_default();
    state.session_stores.lock().await.insert(token.clone(), session.cookie_store.clone());
    // Saving is a convenience; failing to save must not undo a good login.
    if let Err(e) = credentials::remember(app, &session.account, &session.password) {
        eprintln!("[credentials] {e}");
    }
    Ok(PasswordLoginResult::Approved { token, games })
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

    /// Windows「設定 → 協助工具 → 文字大小」的倍率（1.0＝100%，最大 2.25）。
    /// 沒調過的機器連這個值都不存在，所以讀不到一律當 100%。
    ///
    /// ★這個設定**不會**改變 `GetDpiForWindow` 回報的 DPI，所以 tao 算出來的視窗
    /// 尺寸完全不含它；WebView2 卻把它併進自己的縮放——Microsoft 在
    /// WebView2Feedback#3699 的說法是「we tie the text setting with DPI scaling
    /// for both the browser and WebView2」，也就是整頁一起放大而不是只放大字。
    /// 兩邊對不起來，頁面就比視窗大一圈、右邊與下面被裁掉。
    ///
    /// ★只讀一次就記住：拖動帳號瀏覽器時 `relayout_tabs` 會被高頻觸發，每個
    /// `Moved` 都去開一次登錄檔太浪費。副作用剛好就是想要的行為——改完設定要
    /// 重開程式才生效。
    pub fn text_scale_factor() -> f64 {
        static CACHED: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
        *CACHED.get_or_init(|| {
            use winreg::enums::HKEY_CURRENT_USER;
            use winreg::RegKey;
            let percent = RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey(r"Software\Microsoft\Accessibility")
                .ok()
                .and_then(|k| k.get_value::<u32, _>("TextScaleFactor").ok())
                .unwrap_or(100);
            percent.clamp(100, 225) as f64 / 100.0
        })
    }

    /// 工作區的寬高（實體像素）。問不到就是 `None`——呼叫端照樣要把視窗放大，
    /// 只是沒有上限可夾。
    pub fn work_area_size() -> Option<(u32, u32)> {
        let (l, t, r, b) = primary_work_area()?;
        Some(((r - l).max(0) as u32, (b - t).max(0) as u32))
    }

    /// 尺寸夾進工作區。`work` 是 `None`（問不到工作區）就原樣放行。
    pub fn clamp_to_work_area(size: (u32, u32), work: Option<(u32, u32)>) -> (u32, u32) {
        match work {
            Some((mw, mh)) => (size.0.min(mw), size.1.min(mh)),
            None => size,
        }
    }

    /// 視窗要多大才裝得下被放大的頁面。**夾在工作區以內**：文字調到 225% 時
    /// 640 的高度會要 1440，1080p 螢幕根本放不下，寧可切掉一點內容，也不要讓
    /// 視窗大到標題列跑出螢幕外變成抓不到、關不掉。
    pub fn size_for_text_scale(base: (u32, u32), scale: f64, work: Option<(u32, u32)>) -> (u32, u32) {
        let grown = (
            (base.0 as f64 * scale).round() as u32,
            (base.1 as f64 * scale).round() as u32,
        );
        clamp_to_work_area(grown, work)
    }

    #[cfg(test)]
    mod tests {
        use super::{clamp_to_work_area, size_for_text_scale};

        #[test]
        fn the_window_grows_by_the_same_factor_the_page_did() {
            assert_eq!(size_for_text_scale((420, 640), 1.5, Some((1920, 1080))), (630, 960));
        }

        #[test]
        fn a_window_too_tall_for_the_screen_is_clamped_to_the_work_area() {
            // 225% × 640 ＝ 1440，1080p 放不下：寧可切掉內容也不要讓標題列跑出螢幕。
            assert_eq!(size_for_text_scale((420, 640), 2.25, Some((1920, 1040))), (945, 1040));
        }

        #[test]
        fn a_missing_work_area_still_grows_the_window() {
            // 問不到工作區照樣要放大——不放大等於這台機器的顯示問題完全沒修。
            assert_eq!(size_for_text_scale((420, 640), 1.5, None), (630, 960));
        }

        #[test]
        fn clamping_leaves_a_window_that_already_fits_alone() {
            assert_eq!(clamp_to_work_area((630, 960), Some((1920, 1040))), (630, 960));
        }
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
        shell_open_with_args(target, None)
    }

    /// [`shell_open`] with command-line arguments handed to the target.
    pub fn shell_open_with_args(target: &str, args: Option<&str>) -> Result<(), String> {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let target_wide: Vec<u16> = target.encode_utf16().chain(Some(0u16)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0u16)).collect();
        let args_wide: Option<Vec<u16>> =
            args.map(|a| a.encode_utf16().chain(Some(0u16)).collect());
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target_wide.as_ptr(),
                args_wide.as_ref().map_or(std::ptr::null(), |a| a.as_ptr()),
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

/// Warm the game_zone session once for a batch of `launch_uri_of` calls. The
/// per-account request alone is not enough on a session that has not navigated
/// there yet, but the warming is per-session, so a batch pays for it once.
#[tauri::command]
async fn prime_game_zone(
    state: tauri::State<'_, AppState>,
    token: String,
) -> Result<(), String> {
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        stores.get(&token).cloned()
            .ok_or_else(|| "SESSION_EXPIRED".to_string())?
    };
    beanfun::prime_game_zone(&cookie_store, &token)
        .await
        .map_err(map_err)
}

/// One account's launch URI, on a session `prime_game_zone` has already warmed.
#[tauri::command]
async fn launch_uri_of(
    state: tauri::State<'_, AppState>,
    token: String,
    account_sn: String,
) -> Result<String, String> {
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        stores.get(&token).cloned()
            .ok_or_else(|| "SESSION_EXPIRED".to_string())?
    };
    beanfun::launch_uri_for(&cookie_store, &account_sn)
        .await
        .map_err(map_err)
}

// ─── Hidden features ──────────────────────────────────────────────────────────

/// Check a hidden-feature key, returning the feature id it unlocks. Lives in
/// Rust rather than the frontend so the digest ships inside the binary instead
/// of a greppable JS bundle.
#[tauri::command]
fn verify_hidden_key(key: String) -> Option<String> {
    hidden::verify(&key).map(str::to_owned)
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

// ─── Icon ────────────────────────────────────────────────────────────────────

/// Apply the app icon that matches the frontend theme. Fire-and-forget: the icon
/// module never fails, so the frontend has nothing to handle.
///
/// The theme string is the frontend's vocabulary (`neutral` / `dark`); it is
/// translated here so the icon module never learns about it.
#[tauri::command]
fn apply_icon_theme(app: tauri::AppHandle, theme: String) {
    let t = if theme == "dark" {
        icon::IconTheme::Dark
    } else {
        icon::IconTheme::Light
    };
    icon::apply(&app, t);
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
///
/// `/UPDATE` makes the installer overwrite in place instead of running the old
/// uninstaller first. That matters because the uninstaller unpins our shortcuts
/// from the taskbar (Tauri's NSIS template calls `UnpinShortcut` whenever it is
/// not in update mode), and Windows gives us no way to pin them back.
#[tauri::command]
async fn update_app(url: String) -> Result<(), String> {
    let installer = beanfun::download_installer(&url, "KuZe-Login-setup.exe").await.map_err(map_err)?;
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(move || win::shell_open_with_args(&installer, Some("/UPDATE")))
            .await
            .map_err(|e| e.to_string())??;
    }
    #[cfg(not(windows))]
    {
        let _ = installer;
    }
    Ok(())
}

/// Delete the binary left behind by the previous in-place update. Called on
/// startup, by which point the process holding it has exited. A failure here is
/// not worth reporting — the file is harmless and the next launch retries.
#[cfg(windows)]
fn sweep_old_exe() {
    if let Ok(cur) = std::env::current_exe() {
        let _ = std::fs::remove_file(cur.with_extension("old.exe"));
    }
}

/// Point the uninstall entry's version at the binary that is actually running.
///
/// An in-place update never runs the installer, so nothing else refreshes this
/// and Windows' installed-apps list would keep advertising whatever version the
/// user last ran the installer for. Written on every startup so any existing
/// drift heals; the key is only ever updated, never created, so an unpacked
/// build touches nothing.
#[cfg(windows)]
fn sync_installed_version(product: &str, version: &str) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
    };

    // Mirrors the NSIS UNINSTKEY, which is keyed on productName under HKCU
    // because the bundle installs per-user.
    let subkey: Vec<u16> =
        format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{product}")
            .encode_utf16().chain(Some(0u16)).collect();
    let name: Vec<u16> = "DisplayVersion".encode_utf16().chain(Some(0u16)).collect();
    let value: Vec<u16> = version.encode_utf16().chain(Some(0u16)).collect();

    unsafe {
        let mut key = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_SET_VALUE, &mut key) != 0 {
            return;
        }
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr() as *const u8,
            (value.len() * 2) as u32,
        );
        RegCloseKey(key);
    }
}

/// Replace the running executable with a freshly downloaded one and restart.
///
/// Preferred over [`update_app`] because it never runs the NSIS installer, and
/// so never trips the uninstaller that strips our taskbar pins.
///
/// A bare file swap is sufficient only while the exe carries everything it
/// needs. Nothing may be added to `bundle.resources`: an installed-only file
/// never reaches anyone who updates this way. v1.5.0 shipped the themed icons
/// that way and the whole feature was inert until v1.5.1 embedded them.
///
/// The download lands next to the running exe rather than in `%TEMP%` so the
/// final move is a same-volume rename — atomic, and never a half-copied binary.
#[cfg(windows)]
#[tauri::command]
async fn update_app_inplace(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri::Emitter;

    let cur = std::env::current_exe().map_err(|e| format!("找不到程式路徑：{e}"))?;
    let staged = cur.with_extension("new.exe");
    let retired = cur.with_extension("old.exe");

    // A retired binary from an earlier update may still be sitting here if the
    // startup sweep could not remove it; the rename below would fail on it.
    let _ = std::fs::remove_file(&retired);

    // Emit only when the whole-percent figure moves: a 15 MB body arrives in
    // thousands of chunks, and every event is a serialised IPC round trip.
    let mut last_pct = u64::MAX;
    beanfun::download_to(&url, &staged, |done, total| {
        let pct = if total > 0 { done * 100 / total } else { 0 };
        if pct != last_pct {
            last_pct = pct;
            let _ = app.emit("update-progress", (done, total));
        }
    })
    .await
    .map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        map_err(e)
    })?;

    std::fs::rename(&cur, &retired).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        format!("無法置換程式檔：{e}")
    })?;
    if let Err(e) = std::fs::rename(&staged, &cur) {
        // Put the running exe back, or the install is left with no binary at all.
        let _ = std::fs::rename(&retired, &cur);
        let _ = std::fs::remove_file(&staged);
        return Err(format!("無法寫入新版程式：{e}"));
    }

    // Spawned rather than shell-opened: CreateProcess inherits our elevation and
    // skips the SmartScreen prompt ShellExecute would raise on a fresh download.
    std::process::Command::new(&cur)
        .spawn()
        .map_err(|e| format!("無法啟動新版本：{e}"))?;
    app.exit(0);
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
async fn update_app_inplace(_app: tauri::AppHandle, _url: String) -> Result<(), String> {
    Err("就地更新僅支援 Windows".into())
}

// ─── Session Ping ─────────────────────────────────────────────────────────────

/// Ask beanfun whether this token is still logged in. The three states are
/// `beanfun::SessionState`; only `Expired` may clear a token in the UI, since
/// tokens live solely in the frontend store and a wrong verdict costs a QR
/// rescan per account.
#[tauri::command]
async fn ping_session(
    state: tauri::State<'_, AppState>,
    token: String,
) -> Result<SessionState, String> {
    let cookie_store = {
        let stores = state.session_stores.lock().await;
        match stores.get(&token).cloned() {
            // No cookie jar for this token: the session cannot be revived, and
            // every command that needs it already reports SESSION_EXPIRED.
            Some(s) => s,
            None => return Ok(SessionState::Expired),
        }
    };
    Ok(beanfun::check_session(&cookie_store, &token).await)
}

/// Drop a session's cookie jar.
///
/// Called when a re-login replaces an account's token: the old jar is
/// unreachable from the UI from then on, and `session_stores` otherwise holds
/// every jar it ever made for the life of the process. Dropping it also makes
/// the old token answer `Expired` instead of pinging beanfun with a session
/// that has already been superseded.
#[tauri::command]
async fn forget_session(state: tauri::State<'_, AppState>, token: String) -> Result<(), String> {
    state.session_stores.lock().await.remove(&token);
    Ok(())
}

// ─── Refresh key (F5 / Ctrl+R) ────────────────────────────────────────────────

/// Take the refresh keys away from the main window and do nothing with them.
///
/// WebView2's native F5 reloads the whole SPA, and the account list plus its
/// tokens live only in the frontend store (deliberately not persisted), so a
/// reload wipes every account and forces a rescan. Swallowing the key is the
/// point; there is nothing to trigger behind it, because the session check runs
/// on its own every eight minutes and a manual one adds nothing but a way to hit
/// beanfun harder than that.
fn swallow_refresh_keys<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) {
    keyhook::hook_keys(win, |key| {
        // Spelled out rather than pulled from windows-sys so this function stays
        // free of `cfg(windows)` — keyhook already handles the platform split.
        const VK_F5: u32 = 0x74;
        key.vk == VK_F5 || (key.ctrl && key.vk == u32::from(b'R'))
    });
}

// ─── App Entry ────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            pending_qr: Mutex::new(None),
            pending_password: Mutex::new(None),
            session_stores: Mutex::new(HashMap::new()),
        })
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            qr_start, qr_check, password_login_start, password_login_resume, captcha_solve, captcha_cancel, saved_logins, forget_saved_login, reorder_saved_logins, get_otp,
            smart_launch, launch_via_ggm, get_launch_uri, proxy_launch, open_url,
            prime_game_zone, launch_uri_of, verify_hidden_key,
            check_ggm_update, update_ggm, get_game_path, set_game_path, ping_session, forget_session,
            open_account_browser, browser_navigate, browser_tab,
            check_app_update, update_app, update_app_inplace,
            apply_icon_theme
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            app.get_webview_window("main").unwrap().open_devtools();
            #[cfg(windows)]
            {
                sweep_old_exe();
                let pkg = app.package_info();
                sync_installed_version(&pkg.name, &pkg.version.to_string());
            }
            // 開場固定在主螢幕工作區右下角：每次啟動都回這個位置、不記憶拖動後的座標。
            // 用工作區（扣掉工作列）而非螢幕尺寸，否則會被工作列蓋掉一截；用 outer_size
            // （含外框）不是設定檔尺寸，DPI 縮放時才不會少算。
            if let Some(w) = app.get_webview_window("main") {
                // 系統文字大小放大了頁面，卻沒放大視窗（見 win::text_scale_factor），
                // 內容因此溢出被裁掉。把視窗乘回同一個倍率，CSS 視口就回到設計時的
                // 420×640，版面比例一模一樣、字跟著變大——使用者調大文字本來就是要
                // 看得清楚，用 set_zoom 壓回去只會換成「字太小」的抱怨。
                // ★只在啟動時讀一次：改完設定要重開程式才生效。
                #[cfg(windows)]
                {
                    let scale = win::text_scale_factor();
                    if scale > 1.0 {
                        if let Ok(sz) = w.inner_size() {
                            let (nw, nh) = win::size_for_text_scale(
                                (sz.width, sz.height),
                                scale,
                                win::work_area_size(),
                            );
                            let _ = w.set_size(tauri::PhysicalSize::new(nw, nh));
                        }
                    }
                }
                if let (Some((_, _, right, bottom)), Ok(sz)) = (win::primary_work_area(), w.outer_size()) {
                    let x = right - sz.width as i32;
                    let y = bottom - sz.height as i32;
                    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
                }
                swallow_refresh_keys(&w);

                // 關掉主視窗＝結束整個程式。沒有這段的話，只要帳號瀏覽器還開著，
                // event loop 就認為還有視窗活著而不退出——而主視窗一關就再也叫不
                // 回來（沒有系統匣、沒有 single instance、沒有任何 show 回主視窗的
                // 路徑）。帳號瀏覽器留下的幽靈條目更是永遠不會消失，那時連進程都
                // 退不掉，會一直留在背景。
                // ★代價（使用者拍板接受）：exit 不會觸發任何視窗的 CloseRequested，
                // 帳號瀏覽器的視窗幾何（browser-window.json）因此存不到——先關主視窗
                // 的那條路，下次開瀏覽器會回到預設大小位置。
                let exit_handle = app.handle().clone();
                w.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                        exit_handle.exit(0);
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

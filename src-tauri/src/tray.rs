//! 系統匣：設定開了「縮小到通知列」時，主視窗的縮小鈕把視窗藏起來，靠這裡的圖示叫回來。
//! 關閉鈕不歸這裡管——它永遠是結束程式。
//!
//! 圖示只在設定開著的期間存在：開的時候才建、關的時候整顆移除，不用「建好再藏」。
//! tray-icon 0.23 在 Explorer 重啟時會把藏著的圖示無條件加回來、切可見性也不回報
//! 失敗——圖示不存在就沒有這些事。建立本身會回報失敗（通知區域加不進去），正好拿來
//! 擋「視窗藏了卻沒有圖示可以叫回來」。
//!
//! 登入器可以多開：每個實例各有自己的系統匣圖示，各管各的主視窗。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main";

/// 前端啟動後才會送來使用者的選擇；在那之前縮小就是一般的最小化。
static MINIMIZE_TO_TRAY: AtomicBool = AtomicBool::new(false);

fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "顯示主視窗", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "結束", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("久世登入器")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    let icon = tauri::image::Image::from_bytes(crate::icon::ICON_BYTES)?;
    builder.icon(icon).build(app).map(|_| ())
}

/// 開：圖示建成功才記旗標，否則縮小鈕維持一般最小化。關：先收旗標再拆圖示，
/// 中間不會有「旗標還開著、圖示已經沒了」的空檔。
pub fn set_enabled(app: &AppHandle, on: bool) -> Result<(), String> {
    if on {
        if app.tray_by_id(TRAY_ID).is_none() {
            build(app).map_err(|e| format!("通知列圖示建立失敗：{e}"))?;
        }
        MINIMIZE_TO_TRAY.store(true, Ordering::Relaxed);
    } else {
        MINIMIZE_TO_TRAY.store(false, Ordering::Relaxed);
        drop(app.remove_tray_by_id(TRAY_ID));
    }
    Ok(())
}

/// 主視窗縮小鈕的唯一出口：照設定藏進系統匣，或一般最小化到工作列。
///
/// 藏之前一樣先最小化：驗證視窗與 GamaPass 視窗是主視窗「擁有」的，Windows 只在擁有者
/// 最小化時才會連帶收掉它們，單純 hide 會把它們孤零零留在桌面上。叫回來時 `show_main`
/// 的 unminimize 會把它們一起帶回來。
pub fn minimize_main(app: &AppHandle) -> Result<(), String> {
    let w = app.get_webview_window("main").ok_or("找不到主視窗")?;
    w.minimize().map_err(|e| e.to_string())?;
    if MINIMIZE_TO_TRAY.load(Ordering::Relaxed) {
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

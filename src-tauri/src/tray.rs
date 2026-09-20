//! 系統匣：設定開了「縮小到通知列」時，主視窗的縮小鈕把視窗藏起來，靠這裡的圖示叫回來。
//! 關閉鈕不歸這裡管——它永遠是結束程式。
//!
//! 圖示在啟動時就建好、預設藏著，開關只切它的可見性——這樣圖標模組啟動時套主題圖示
//! 不必管這個設定當下開了沒。
//!
//! 登入器可以多開：每個實例各有自己的系統匣圖示，各管各的主視窗。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main";

/// 前端啟動後才會送來使用者的選擇；在那之前縮小就是一般的最小化。
static MINIMIZE_TO_TRAY: AtomicBool = AtomicBool::new(false);

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "顯示主視窗", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "結束", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
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
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?.set_visible(false)
}

/// 先切圖示再記旗標：圖示沒出來就不能讓縮小鈕開始藏視窗，否則主視窗一藏就再也叫不回來。
pub fn set_enabled(app: &AppHandle, on: bool) -> Result<(), String> {
    // 可見性跟旗標永遠同步，沒變就別碰：對已經藏著的圖示再藏一次，Windows 會回錯
    if MINIMIZE_TO_TRAY.load(Ordering::Relaxed) == on {
        return Ok(());
    }
    let tray = app.tray_by_id(TRAY_ID).ok_or("系統匣圖示不存在")?;
    tray.set_visible(on).map_err(|e| e.to_string())?;
    MINIMIZE_TO_TRAY.store(on, Ordering::Relaxed);
    Ok(())
}

/// 主視窗縮小鈕的唯一出口：照設定藏進系統匣，或一般最小化到工作列。
pub fn minimize_main(app: &AppHandle) -> Result<(), String> {
    let w = app.get_webview_window("main").ok_or("找不到主視窗")?;
    if MINIMIZE_TO_TRAY.load(Ordering::Relaxed) {
        w.hide()
    } else {
        w.minimize()
    }
    .map_err(|e| e.to_string())
}

/// 圖標模組套主題時順手換掉系統匣那一張。盡力而為，跟視窗圖示同一個待遇。
pub fn set_icon(app: &AppHandle, icon: tauri::image::Image<'_>) -> tauri::Result<()> {
    match app.tray_by_id(TRAY_ID) {
        Some(tray) => tray.set_icon(Some(icon)),
        None => Ok(()),
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

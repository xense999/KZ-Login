//! Keyboard interception for our own webviews.
//!
//! WebView2's `AcceleratorKeyPressed` fires *before* the browser acts on a key,
//! and `SetHandled(true)` is the only guaranteed way to suppress a built-in
//! shortcut — a page-level `preventDefault` is not, and a tab webview loading a
//! remote site has no IPC to reach us with anyway. Everything that needs to take
//! a key away from WebView2 goes through [`hook_keys`].

use tauri::{Runtime, WebviewWindow};

/// A key-down, already stripped of auto-repeat.
pub struct Key {
    /// Win32 virtual-key code. Letters arrive as their uppercase ASCII value.
    pub vk: u32,
    pub ctrl: bool,
}

/// Route this webview's key-downs to `on_key`; returning `true` consumes the key
/// so WebView2 never sees it.
///
/// `on_key` runs on the WebView2 callback thread: it must not block and must not
/// re-enter WebView2 (creating or closing a window from here is re-entrancy and
/// will crash). Do the work on another thread.
#[cfg(windows)]
pub fn hook_keys<R: Runtime, F>(win: &WebviewWindow<R>, on_key: F)
where
    F: Fn(Key) -> bool + Send + 'static,
{
    use webview2_com::AcceleratorKeyPressedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_PHYSICAL_KEY_STATUS,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};

    let label = win.label().to_string();
    let _ = win.with_webview(move |platform| unsafe {
        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_controller, args| {
            let Some(args) = args else { return Ok(()) };

            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
            let _ = args.KeyEventKind(&mut kind);
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN {
                return Ok(());
            }

            // Holding a key raises this event once per WM_KEYDOWN. Act on the
            // first one only, or a held key queues one action per repeat tick.
            let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
            if args.PhysicalKeyStatus(&mut status).is_ok() && status.WasKeyDown.as_bool() {
                return Ok(());
            }

            let mut vk = 0u32;
            let _ = args.VirtualKey(&mut vk);
            let key = Key {
                vk,
                ctrl: (GetKeyState(VK_CONTROL as i32) as u16 & 0x8000) != 0,
            };
            if on_key(key) {
                let _ = args.SetHandled(true);
            }
            Ok(())
        }));

        let mut token = 0i64;
        if let Err(e) = platform.controller().add_AcceleratorKeyPressed(&handler, &mut token) {
            eprintln!("[keyhook] failed to hook keys on {label}: {e}");
        }
    });
}

#[cfg(not(windows))]
pub fn hook_keys<R: Runtime, F>(_win: &WebviewWindow<R>, _on_key: F)
where
    F: Fn(Key) -> bool + Send + 'static,
{
}

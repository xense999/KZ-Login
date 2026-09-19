//! Laying a borderless child window over one region of the main window.
//!
//! Two flows need a real WebView on beanfun's own page — the reCAPTCHA
//! checkbox and the GamaPass sign-in — and neither may live inside our page:
//! beanfun refuses to be framed, and its token is only valid from its own
//! origin. A separate window would read as a different program, so both are
//! instead pinned over the strip of the main window between the title bar and
//! the bottom bar, and follow it when it moves.

use serde::Deserialize;
use tauri::{PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

/// The app theme's colours, read from the main window's CSS so a window laid
/// over it matches without a second copy of the palette living in Rust.
#[derive(Debug, Clone, Deserialize)]
pub struct Palette {
    pub bg: String,
    pub text: String,
    pub dark: bool,
}

/// Where the child window goes, in CSS pixels of the main window's client
/// area. Measured by the page that owns that strip — no size is written here
/// or anywhere else in Rust.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// WebView2 flags both windows need. Without them the widget treats the
/// WebView as automation and serves endless image challenges, and tracking
/// prevention starves cross-origin frames of the storage they need.
pub const BROWSER_ARGS: &str = "--disable-blink-features=AutomationControlled \
     --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,ThirdPartyStoragePartitioning,PartitionedCookies,msEdgeTrackingPrevention";

/// Put `window` over `region` of `main`. Called every poll tick, not only on
/// change — that is what makes the child follow a dragged main window.
pub fn place<R: Runtime>(window: &WebviewWindow<R>, main: &WebviewWindow<R>, region: Region) {
    let (Ok(origin), Ok(scale)) = (main.inner_position(), main.scale_factor()) else {
        return;
    };
    // `region` 是主視窗頁面量出來的 CSS px，而那一頁被系統的文字大小一起放大了
    // （見 `win::text_scale_factor`）——DPI 的倍率裡不含它，要另外乘回去。
    #[cfg(windows)]
    let scale = scale * crate::win::text_scale_factor();
    let pos = PhysicalPosition::new(
        origin.x + (region.x * scale).round() as i32,
        origin.y + (region.y * scale).round() as i32,
    );
    let size = PhysicalSize::new(
        (region.width * scale).round() as u32,
        (region.height * scale).round() as u32,
    );
    if window.outer_position().ok() != Some(pos) {
        let _ = window.set_position(pos);
    }
    if window.inner_size().ok() != Some(size) {
        let _ = window.set_size(size);
    }
}

#[cfg(windows)]
pub fn disable_tracking_prevention<R: Runtime>(window: &WebviewWindow<R>) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Profile3, ICoreWebView2_13, COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE,
    };
    use windows_core::Interface;

    let _ = window.with_webview(|platform| unsafe {
        let profile = platform
            .controller()
            .CoreWebView2()
            .and_then(|core| core.cast::<ICoreWebView2_13>())
            .and_then(|core| core.Profile())
            .and_then(|profile| profile.cast::<ICoreWebView2Profile3>());
        match profile {
            Ok(p) => {
                let _ = p.SetPreferredTrackingPreventionLevel(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE);
            }
            Err(e) => eprintln!("[overlay] tracking prevention stays on: {e}"),
        }
    });
}

#[cfg(not(windows))]
pub fn disable_tracking_prevention<R: Runtime>(_window: &WebviewWindow<R>) {}

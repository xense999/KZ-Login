//! reCAPTCHA solving window for the password login.
//!
//! beanfun's reCAPTCHA token is only accepted from its own origin, so it can
//! neither be produced over HTTP nor drawn inside our app page. This opens a
//! borderless window on beanfun's login page, laid over one region of the main
//! window (the strip between the title bar and the bottom bar), and covers the
//! page with an overlay that shows nothing but the checkbox. The image challenge
//! is scaled down to fit that region rather than letting the window grow. The
//! token comes back through the URL fragment: the page's CSP keeps app IPC out,
//! and the window deliberately has no capability anyway.

use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const LABEL_PREFIX: &str = "captcha-";
const TOKEN_FRAGMENT: &str = "kz-captcha=";
/// Short enough that the window keeps up when the main window is dragged.
const POLL_INTERVAL: Duration = Duration::from_millis(80);
const TIMEOUT: Duration = Duration::from_secs(180);

/// A fixed label would collide: tauri only forgets a label once the old
/// window's `Destroyed` event has gone through the event loop.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The app theme's colours, read from the main window's CSS so the overlay
/// matches without a second copy of the palette.
#[derive(Debug, Deserialize)]
pub struct Palette {
    pub bg: String,
    pub text: String,
    pub dark: bool,
}

/// Where the checkbox goes, in CSS pixels of the main window's client area.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Show the checkbox and wait for the user. `None` means cancelled (see
/// [`cancel`]), timed out, or the window went away.
pub async fn solve<R: Runtime>(
    app: &AppHandle<R>,
    page_url: &str,
    site_key: &str,
    palette: &Palette,
    region: Region,
) -> Result<Option<String>, String> {
    let main = app.get_webview_window("main").ok_or("找不到主視窗")?;
    let url: Url = page_url.parse().map_err(|e| format!("驗證頁網址錯誤：{e}"))?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("取不到資料夾：{e}"))?
        .join("captcha-webview");

    cancel(app);
    let label = format!("{LABEL_PREFIX}{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .visible(false)
        .owner(&main)
        .map_err(|e| e.to_string())?
        // Its own WebView2 environment: the browser args differ from the main
        // window's, and one user-data folder cannot host both. The folder is
        // reused every time, so nothing piles up.
        .data_directory(data_dir)
        // Without these the widget takes the WebView for automation and serves
        // endless image challenges, and tracking prevention starves the Google
        // iframe of the storage it needs.
        .additional_browser_args(
            "--disable-blink-features=AutomationControlled \
             --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,ThirdPartyStoragePartitioning,PartitionedCookies,msEdgeTrackingPrevention",
        )
        .initialization_script(&overlay_script(site_key, palette))
        .build()
        .map_err(|e| format!("驗證視窗開不起來：{e}"))?;

    place(&window, &main, region);
    disable_tracking_prevention(&window);
    let _ = window.show();
    let _ = window.set_focus();

    let started = Instant::now();
    let token = loop {
        if started.elapsed() >= TIMEOUT {
            break None;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
        let Some(window) = app.get_webview_window(&label) else { break None };
        if let Some(token) = window.url().ok().as_ref().and_then(Url::fragment).and_then(read_token) {
            break Some(token);
        }
        // Every tick, not only on change: this is what makes the window follow
        // the main window when it is dragged.
        place(&window, &main, region);
    };

    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.destroy();
    }
    Ok(token)
}

/// Close any open captcha window; a pending [`solve`] then returns `None`.
pub fn cancel<R: Runtime>(app: &AppHandle<R>) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(LABEL_PREFIX) {
            let _ = window.destroy();
        }
    }
}

fn place<R: Runtime>(window: &WebviewWindow<R>, main: &WebviewWindow<R>, region: Region) {
    let (Ok(origin), Ok(scale)) = (main.inner_position(), main.scale_factor()) else {
        return;
    };
    let pos = PhysicalPosition::new(
        origin.x + (region.x * scale).round() as i32,
        origin.y + (region.y * scale).round() as i32,
    );
    let size = PhysicalSize::new((region.width * scale).round() as u32, (region.height * scale).round() as u32);
    if window.outer_position().ok() != Some(pos) {
        let _ = window.set_position(pos);
    }
    if window.inner_size().ok() != Some(size) {
        let _ = window.set_size(size);
    }
}

fn read_token(fragment: &str) -> Option<String> {
    fragment
        .strip_prefix(TOKEN_FRAGMENT)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

#[cfg(windows)]
fn disable_tracking_prevention<R: Runtime>(window: &WebviewWindow<R>) {
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
            Err(e) => eprintln!("[captcha] tracking prevention stays on: {e}"),
        }
    });
}

#[cfg(not(windows))]
fn disable_tracking_prevention<R: Runtime>(_window: &WebviewWindow<R>) {}

fn overlay_script(site_key: &str, palette: &Palette) -> String {
    let json = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    OVERLAY_JS
        .replace("__SITE_KEY__", &json(site_key))
        .replace("__BG__", &json(&palette.bg))
        .replace("__TEXT__", &json(&palette.text))
        .replace("__DARK__", if palette.dark { "true" } else { "false" })
        .replace("__TOKEN_FRAGMENT__", &json(TOKEN_FRAGMENT))
}

/// Runs on every document in the window, but only acts on beanfun's login
/// host. The widget is rendered by us into our own container rather than
/// fished out of beanfun's page, so a redesign of their page cannot hide it.
///
/// The image challenge iframe is laid out by reCAPTCHA at its own size (about
/// 400×580) and usually parked off-screen; it is pinned to the middle of the
/// window and scaled down to fit. It is only ever visible while the challenge
/// is open, because reCAPTCHA hides its container otherwise.
const OVERLAY_JS: &str = r##"(() => {
  if (location.hostname !== "login.beanfun.com") return;
  try { Object.defineProperty(navigator, "webdriver", { get: () => false }); } catch (e) {}
  const SITE_KEY = __SITE_KEY__;
  const C = { bg: __BG__, text: __TEXT__ };
  const DARK = __DARK__;
  const TOKEN = __TOKEN_FRAGMENT__;
  const MARGIN = 6;

  const style = document.createElement("style");
  style.textContent =
    "html,body{background:transparent!important;overflow:hidden!important}" +
    // Below the image challenge, which reCAPTCHA stacks at about 2e9.
    "#kz-cap{position:fixed;inset:0;z-index:999999;display:flex;flex-direction:column;" +
    "align-items:center;justify-content:center;gap:14px;background:" + C.bg + ";color:" + C.text + ";" +
    "font:13px 'Segoe UI','Microsoft JhengHei',system-ui,sans-serif}" +
    "#kz-cap-box{min-height:78px;display:flex;align-items:center;justify-content:center}" +
    "#kz-cap-hint{opacity:.7;text-align:center;line-height:1.6}";

  const root = document.createElement("div");
  root.id = "kz-cap";
  root.innerHTML = '<div id="kz-cap-hint">載入驗證中…</div><div id="kz-cap-box"></div>';

  const mount = () => {
    if (document.getElementById("kz-cap")) return;
    document.head.appendChild(style);
    document.body.appendChild(root);
    render();
    setInterval(fitChallenge, 150);
  };

  const fitChallenge = () => {
    const frame = document.querySelector("iframe[src*='recaptcha'][src*='bframe']");
    if (!frame || !frame.offsetWidth || !frame.offsetHeight) return;
    const w = frame.offsetWidth, h = frame.offsetHeight;
    const vw = window.innerWidth, vh = window.innerHeight;
    const k = Math.min(1, (vw - MARGIN * 2) / w, (vh - MARGIN * 2) / h);
    const set = (prop, value) => frame.style.setProperty(prop, value, "important");
    set("position", "fixed");
    set("left", Math.round((vw - w * k) / 2) + "px");
    set("top", Math.round((vh - h * k) / 2) + "px");
    set("transform", "scale(" + k + ")");
    set("transform-origin", "0 0");
  };

  let rendered = false;
  const render = () => {
    if (rendered) return;
    const ent = window.grecaptcha && window.grecaptcha.enterprise;
    if (!ent || typeof ent.render !== "function") {
      if (!document.querySelector("script[src*='recaptcha/enterprise.js']")) {
        const s = document.createElement("script");
        s.src = "https://www.google.com/recaptcha/enterprise.js?render=explicit";
        s.async = true;
        document.head.appendChild(s);
      }
      setTimeout(render, 200);
      return;
    }
    ent.ready(() => {
      if (rendered) return;
      rendered = true;
      root.querySelector("#kz-cap-hint").textContent = "請勾選「我不是機器人」";
      ent.render(root.querySelector("#kz-cap-box"), {
        sitekey: SITE_KEY,
        theme: DARK ? "dark" : "light",
        callback: (token) => { location.hash = TOKEN + token; },
      });
    });
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", mount, { once: true });
  } else {
    mount();
  }
})();"##;

#[cfg(test)]
mod tests {
    use super::read_token;

    #[test]
    fn reads_the_token_the_overlay_writes() {
        assert_eq!(read_token("kz-captcha=abc-_123").as_deref(), Some("abc-_123"));
    }

    /// beanfun's own page may set fragments of its own; none of them is an answer.
    #[test]
    fn ignores_other_fragments() {
        assert!(read_token("").is_none());
        assert!(read_token("kz-captcha=").is_none());
        assert!(read_token("top").is_none());
    }
}

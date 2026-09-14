//! reCAPTCHA solving window for the password login.
//!
//! beanfun's reCAPTCHA token is only accepted from its own origin, so it can
//! neither be produced over HTTP nor drawn inside our app page. This opens a
//! borderless window on beanfun's login page, laid over one region of the main
//! window (the strip between the title bar and the bottom bar), and covers the
//! page with an overlay that shows nothing but the checkbox. The token comes
//! back through the URL fragment: the page's CSP keeps app IPC out, and the
//! window deliberately has no capability anyway.

use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const LABEL_PREFIX: &str = "captcha-";
const TOKEN_FRAGMENT: &str = "kz-captcha=";
/// The image challenge does not fit the region; while it is open the overlay
/// asks for the whole main window through this fragment, and gives it back.
const FULL_FRAGMENT: &str = "kz-captcha-size=full";
const REGION_FRAGMENT: &str = "kz-captcha-size=region";
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

#[derive(Debug, PartialEq)]
enum Signal {
    Token(String),
    Full,
    Region,
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
        .initialization_script(&overlay_script(site_key, palette, region))
        .build()
        .map_err(|e| format!("驗證視窗開不起來：{e}"))?;

    let mut full = false;
    place(&window, &main, region, full);
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
        match window.url().ok().as_ref().and_then(Url::fragment).and_then(read_fragment) {
            Some(Signal::Token(token)) => break Some(token),
            Some(Signal::Full) => full = true,
            Some(Signal::Region) => full = false,
            None => {}
        }
        // Every tick, not only on change: this is what makes the window follow
        // the main window when it is dragged.
        place(&window, &main, region, full);
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

fn place<R: Runtime>(window: &WebviewWindow<R>, main: &WebviewWindow<R>, region: Region, full: bool) {
    let (Ok(origin), Ok(client), Ok(scale)) = (main.inner_position(), main.inner_size(), main.scale_factor())
    else {
        return;
    };
    let (pos, size) = if full {
        (origin, client)
    } else {
        (
            PhysicalPosition::new(origin.x + (region.x * scale).round() as i32, origin.y + (region.y * scale).round() as i32),
            PhysicalSize::new((region.width * scale).round() as u32, (region.height * scale).round() as u32),
        )
    };
    if window.outer_position().ok() != Some(pos) {
        let _ = window.set_position(pos);
    }
    if window.inner_size().ok() != Some(size) {
        let _ = window.set_size(size);
    }
}

fn read_fragment(fragment: &str) -> Option<Signal> {
    match fragment {
        FULL_FRAGMENT => Some(Signal::Full),
        REGION_FRAGMENT => Some(Signal::Region),
        _ => fragment
            .strip_prefix(TOKEN_FRAGMENT)
            .filter(|t| !t.is_empty())
            .map(|t| Signal::Token(t.to_owned())),
    }
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

fn overlay_script(site_key: &str, palette: &Palette, region: Region) -> String {
    let json = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    OVERLAY_JS
        .replace("__SITE_KEY__", &json(site_key))
        .replace("__BG__", &json(&palette.bg))
        .replace("__TEXT__", &json(&palette.text))
        .replace("__DARK__", if palette.dark { "true" } else { "false" })
        .replace("__REGION__", &format!("[{},{},{},{}]", region.x, region.y, region.width, region.height))
        .replace("__TOKEN_FRAGMENT__", &json(TOKEN_FRAGMENT))
        .replace("__FULL_FRAGMENT__", &json(FULL_FRAGMENT))
        .replace("__REGION_FRAGMENT__", &json(REGION_FRAGMENT))
}

/// Runs on every document in the window, but only acts on beanfun's login
/// host. The widget is rendered by us into our own container rather than
/// fished out of beanfun's page, so a redesign of their page cannot hide it.
///
/// When the image challenge opens, the window grows to the whole main window;
/// the box is then pinned at the region's offset so the checkbox does not jump
/// under the user's cursor.
const OVERLAY_JS: &str = r##"(() => {
  if (location.hostname !== "login.beanfun.com") return;
  try { Object.defineProperty(navigator, "webdriver", { get: () => false }); } catch (e) {}
  const SITE_KEY = __SITE_KEY__;
  const C = { bg: __BG__, text: __TEXT__ };
  const DARK = __DARK__;
  const [RX, RY, RW, RH] = __REGION__;
  const TOKEN = __TOKEN_FRAGMENT__, FULL = __FULL_FRAGMENT__, REGION = __REGION_FRAGMENT__;

  const style = document.createElement("style");
  style.textContent =
    "html,body{background:transparent!important;overflow:hidden!important}" +
    // Both layers stay below the image challenge, which reCAPTCHA stacks at about 2e9.
    "#kz-cap-back{position:fixed;inset:0;z-index:999998;background:" + C.bg + "}" +
    "#kz-cap{position:fixed;inset:0;z-index:999999;display:flex;flex-direction:column;" +
    "align-items:center;justify-content:center;gap:14px;background:" + C.bg + ";color:" + C.text + ";" +
    "font:13px 'Segoe UI','Microsoft JhengHei',system-ui,sans-serif}" +
    "html.kz-full #kz-cap{inset:auto;left:" + RX + "px;top:" + RY + "px;width:" + RW + "px;height:" + RH + "px}" +
    "#kz-cap-box{min-height:78px;display:flex;align-items:center;justify-content:center}" +
    "#kz-cap-hint{opacity:.7;text-align:center;line-height:1.6}";

  const back = document.createElement("div");
  back.id = "kz-cap-back";
  const root = document.createElement("div");
  root.id = "kz-cap";
  root.innerHTML = '<div id="kz-cap-hint">載入驗證中…</div><div id="kz-cap-box"></div>';

  const mount = () => {
    if (document.getElementById("kz-cap")) return;
    document.head.appendChild(style);
    document.body.appendChild(back);
    document.body.appendChild(root);
    render();
    setInterval(watchChallenge, 150);
  };

  // The challenge iframe always exists once rendered; it is "open" when no
  // ancestor hides it or parks it off-screen.
  let full = false;
  const watchChallenge = () => {
    const frame = document.querySelector("iframe[src*='recaptcha'][src*='bframe']");
    let open = !!frame;
    for (let el = frame; open && el && el !== document.body; el = el.parentElement) {
      const cs = getComputedStyle(el);
      if (cs.visibility === "hidden" || cs.display === "none" || el.getBoundingClientRect().top < -1000) open = false;
    }
    if (open === full || location.hash.startsWith("#" + TOKEN)) return;
    full = open;
    document.documentElement.classList.toggle("kz-full", full);
    location.hash = full ? FULL : REGION;
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
    use super::{read_fragment, Signal};

    #[test]
    fn reads_what_the_overlay_writes() {
        assert_eq!(read_fragment("kz-captcha=abc-_123"), Some(Signal::Token("abc-_123".into())));
        assert_eq!(read_fragment("kz-captcha-size=full"), Some(Signal::Full));
        assert_eq!(read_fragment("kz-captcha-size=region"), Some(Signal::Region));
    }

    /// beanfun's own page may set fragments of its own; none of them is an answer.
    #[test]
    fn ignores_other_fragments() {
        assert!(read_fragment("").is_none());
        assert!(read_fragment("kz-captcha=").is_none());
        assert!(read_fragment("top").is_none());
    }
}

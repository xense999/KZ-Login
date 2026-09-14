//! reCAPTCHA solving window for the password login.
//!
//! beanfun's reCAPTCHA token is only accepted from its own origin, so it can
//! neither be produced over HTTP nor drawn inside our app page. This opens a
//! borderless window on beanfun's login page, right on top of the main window,
//! and covers the page with an overlay that shows nothing but the checkbox. The
//! token comes back through the URL fragment: the page's CSP keeps app IPC out,
//! and the window deliberately has no capability anyway.

use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindowBuilder};

const LABEL_PREFIX: &str = "captcha-";
const TOKEN_FRAGMENT: &str = "kz-captcha=";
const CANCEL_FRAGMENT: &str = "kz-captcha-cancel";
const POLL_INTERVAL: Duration = Duration::from_millis(300);
const TIMEOUT: Duration = Duration::from_secs(180);

/// A fixed label would collide: tauri only forgets a label once the old
/// window's `Destroyed` event has gone through the event loop.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The app theme's colours, read from the main window's CSS so the overlay
/// matches without a second copy of the palette.
#[derive(Debug, Deserialize)]
pub struct Palette {
    pub bg: String,
    pub surface: String,
    pub text: String,
    pub border: String,
    pub dark: bool,
}

enum Outcome {
    Token(String),
    Cancelled,
}

/// Show the checkbox and wait for the user. `None` means cancelled, timed out,
/// or the window could not be opened — all of which the caller treats alike.
pub async fn solve<R: Runtime>(
    app: &AppHandle<R>,
    page_url: &str,
    site_key: &str,
    palette: &Palette,
) -> Result<Option<String>, String> {
    let main = app.get_webview_window("main").ok_or("找不到主視窗")?;
    let url: Url = page_url.parse().map_err(|e| format!("驗證頁網址錯誤：{e}"))?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("取不到資料夾：{e}"))?
        .join("captcha-webview");

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

    // Cover the main window exactly; that also keeps it from being dragged away
    // from under the overlay while the user is solving.
    if let (Ok(pos), Ok(size)) = (main.outer_position(), main.outer_size()) {
        let _ = window.set_position(pos);
        let _ = window.set_size(size);
    }
    disable_tracking_prevention(&window);
    let _ = window.show();
    let _ = window.set_focus();

    let outcome = wait_for_outcome(app, &label).await;
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.destroy();
    }
    Ok(match outcome {
        Some(Outcome::Token(token)) => Some(token),
        Some(Outcome::Cancelled) | None => None,
    })
}

async fn wait_for_outcome<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<Outcome> {
    let started = Instant::now();
    while started.elapsed() < TIMEOUT {
        tokio::time::sleep(POLL_INTERVAL).await;
        let window = app.get_webview_window(label)?;
        let Ok(url) = window.url() else { continue };
        if let Some(outcome) = url.fragment().and_then(read_fragment) {
            return Some(outcome);
        }
    }
    None
}

fn read_fragment(fragment: &str) -> Option<Outcome> {
    if fragment == CANCEL_FRAGMENT {
        return Some(Outcome::Cancelled);
    }
    fragment
        .strip_prefix(TOKEN_FRAGMENT)
        .filter(|t| !t.is_empty())
        .map(|t| Outcome::Token(t.to_owned()))
}

#[cfg(windows)]
fn disable_tracking_prevention<R: Runtime>(window: &tauri::WebviewWindow<R>) {
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
fn disable_tracking_prevention<R: Runtime>(_window: &tauri::WebviewWindow<R>) {}

fn overlay_script(site_key: &str, palette: &Palette) -> String {
    let json = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    OVERLAY_JS
        .replace("__SITE_KEY__", &json(site_key))
        .replace("__BG__", &json(&palette.bg))
        .replace("__SURFACE__", &json(&palette.surface))
        .replace("__TEXT__", &json(&palette.text))
        .replace("__BORDER__", &json(&palette.border))
        .replace("__DARK__", if palette.dark { "true" } else { "false" })
        .replace("__TOKEN_FRAGMENT__", &json(TOKEN_FRAGMENT))
        .replace("__CANCEL_FRAGMENT__", &json(CANCEL_FRAGMENT))
}

/// Runs on every document in the window, but only acts on beanfun's login
/// host. The widget is rendered by us into our own container rather than
/// fished out of beanfun's page, so a redesign of their page cannot hide it.
const OVERLAY_JS: &str = r##"(() => {
  if (location.hostname !== "login.beanfun.com") return;
  try { Object.defineProperty(navigator, "webdriver", { get: () => false }); } catch (e) {}
  const SITE_KEY = __SITE_KEY__;
  const C = { bg: __BG__, surface: __SURFACE__, text: __TEXT__, border: __BORDER__ };
  const DARK = __DARK__;
  const TOKEN = __TOKEN_FRAGMENT__, CANCEL = __CANCEL_FRAGMENT__;

  const style = document.createElement("style");
  style.textContent =
    // Below the image challenge, which reCAPTCHA stacks at about 2e9.
    "html,body{background:transparent!important;overflow:hidden!important}" +
    "#kz-cap{position:fixed;inset:0;z-index:999999;display:flex;flex-direction:column;" +
    "background:" + C.bg + ";border:1px solid " + C.border + ";border-radius:14px;" +
    "font:13px 'Segoe UI','Microsoft JhengHei',system-ui,sans-serif;color:" + C.text + "}" +
    "#kz-cap-bar{height:42px;display:flex;align-items:center;justify-content:center;position:relative;opacity:.75}" +
    "#kz-cap-body{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:16px;padding:0 18px}" +
    "#kz-cap-box{min-height:78px;display:flex;align-items:center;justify-content:center}" +
    "#kz-cap-hint{opacity:.7;text-align:center;line-height:1.6}" +
    "#kz-cap-cancel{margin:0 18px 18px;padding:11px;border-radius:10px;cursor:pointer;" +
    "background:" + C.surface + ";border:1px solid " + C.border + ";color:" + C.text + ";font:inherit;font-weight:500}";

  const root = document.createElement("div");
  root.id = "kz-cap";
  root.innerHTML =
    '<div id="kz-cap-bar">驗證</div>' +
    '<div id="kz-cap-body"><div id="kz-cap-hint">載入驗證中…</div><div id="kz-cap-box"></div></div>' +
    '<button id="kz-cap-cancel" type="button">取消</button>';

  const finish = (frag) => { location.hash = frag; };

  const mount = () => {
    if (document.getElementById("kz-cap")) return;
    document.head.appendChild(style);
    document.body.appendChild(root);
    root.querySelector("#kz-cap-cancel").addEventListener("click", () => finish(CANCEL));
    render();
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
        callback: (token) => finish(TOKEN + token),
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
    use super::{read_fragment, Outcome};

    #[test]
    fn reads_what_the_overlay_writes() {
        assert!(matches!(read_fragment("kz-captcha=abc-_123"), Some(Outcome::Token(t)) if t == "abc-_123"));
        assert!(matches!(read_fragment("kz-captcha-cancel"), Some(Outcome::Cancelled)));
    }

    /// beanfun's own page may set fragments of its own; none of them is an answer.
    #[test]
    fn ignores_other_fragments() {
        assert!(read_fragment("").is_none());
        assert!(read_fragment("kz-captcha=").is_none());
        assert!(read_fragment("top").is_none());
    }
}

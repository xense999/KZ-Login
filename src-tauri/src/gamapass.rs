//! GamaPass login window.
//!
//! A GamaPass account cannot be signed in over HTTP the way a beanfun account
//! can — the password, and whatever second factor the account carries, belong
//! to Gamania's own page. So this opens that page in a window of its own and
//! lets the user sign in there; we never see the credentials.
//!
//! The window is borderless and pinned over the login page's own content area
//! (see `overlay`), so signing in reads as part of the app rather than as a
//! browser that appeared out of nowhere.
//!
//! What comes back is not a token. The login is tied to the `pSKey` the window
//! was opened with, so once the portal takes over the page, the caller finishes
//! the same way a QR login does — with `beanfun::complete_login` on the client
//! that minted that key. Reading cookies out of the webview is therefore
//! unnecessary.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindowBuilder};

use crate::overlay::{self, Region};

const LABEL_PREFIX: &str = "gamapass-";
/// Short enough that the window keeps up when the main window is dragged.
const POLL_INTERVAL: Duration = Duration::from_millis(80);
/// Long enough to read a mail or open an authenticator app on the way through.
const TIMEOUT: Duration = Duration::from_secs(600);

/// Hosts that mean the login is done. The sign-in itself wanders off to
/// Gamania's own domains, so anything that is not the login page cannot be the
/// signal — only arriving at the portal can.
const PORTAL_HOSTS: &[&str] = &["tw.beanfun.com", "tw.newlogin.beanfun.com"];

/// A fixed label would collide: tauri only forgets a label once the old
/// window's `Destroyed` event has gone through the event loop.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// How a [`wait_for_login`] ended.
pub enum Outcome {
    /// The portal took over the page — the caller should finish the login.
    Completed,
    /// The window was closed, or nothing happened for [`TIMEOUT`].
    Cancelled,
}

/// Open beanfun's login page for `skey` and wait until the portal takes over.
pub async fn wait_for_login<R: Runtime>(
    app: &AppHandle<R>,
    skey: &str,
    region: Region,
) -> Result<Outcome, String> {
    let main = app.get_webview_window("main").ok_or("找不到主視窗")?;
    let url: Url = format!("https://login.beanfun.com/Login/Index?pSKey={skey}")
        .parse()
        .map_err(|e| format!("登入頁網址錯誤：{e}"))?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("取不到資料夾：{e}"))?
        .join("gamapass-webview");

    cancel(app);
    let label = format!("{LABEL_PREFIX}{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .visible(false)
        .owner(&main)
        .map_err(|e| e.to_string())?
        // Its own WebView2 environment, like the captcha window: one user-data
        // folder cannot host two. Reused every time, so nothing piles up.
        .data_directory(data_dir)
        .additional_browser_args(overlay::BROWSER_ARGS)
        .build()
        .map_err(|e| format!("登入視窗開不起來：{e}"))?;

    overlay::place(&window, &main, region);
    overlay::disable_tracking_prevention(&window);
    let _ = window.show();
    let _ = window.set_focus();

    let started = Instant::now();
    let outcome = loop {
        if started.elapsed() >= TIMEOUT {
            break Outcome::Cancelled;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
        // The user closing the window is the cancel gesture; there is no other.
        let Some(window) = app.get_webview_window(&label) else {
            break Outcome::Cancelled;
        };
        if window.url().ok().is_some_and(|u| at_portal(&u)) {
            break Outcome::Completed;
        }
        overlay::place(&window, &main, region);
    };

    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.destroy();
    }
    Ok(outcome)
}

/// Close an open GamaPass window; a pending [`wait_for_login`] then cancels.
pub fn cancel<R: Runtime>(app: &AppHandle<R>) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(LABEL_PREFIX) {
            let _ = window.destroy();
        }
    }
}

fn at_portal(url: &Url) -> bool {
    url.host_str()
        .is_some_and(|h| PORTAL_HOSTS.iter().any(|p| h.eq_ignore_ascii_case(p)))
}

#[cfg(test)]
mod tests {
    use super::at_portal;
    use tauri::Url;

    fn url(s: &str) -> Url {
        s.parse().unwrap()
    }

    #[test]
    fn the_login_page_itself_is_not_the_signal() {
        assert!(!at_portal(&url("https://login.beanfun.com/Login/Index?pSKey=abc")));
    }

    #[test]
    fn gamania_sign_in_detours_are_not_the_signal() {
        // The sign-in leaves beanfun entirely on the way through; treating any
        // departure from the login page as success would finish far too early.
        assert!(!at_portal(&url("https://tw.gamania.com/login")));
    }

    #[test]
    fn arriving_at_the_portal_is_the_signal() {
        assert!(at_portal(&url("https://tw.newlogin.beanfun.com/checkin_step2.aspx?skey=abc")));
        assert!(at_portal(&url("https://tw.beanfun.com/beanfun_block/bflogin/default.aspx")));
    }
}

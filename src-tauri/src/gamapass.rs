//! GamaPass login window.
//!
//! A GamaPass account cannot be signed in over HTTP the way a beanfun account
//! can — the password check, the reCAPTCHA token and the passkey all belong to
//! Gamania's own page and only work on their origin. So the page is loaded in a
//! window of its own; what we control is whether anyone has to look at it.
//!
//! The window is never shown while the script is working: what the user typed
//! into our own form is put into Gamania's fields, and with a password the
//! whole login can finish without their site ever appearing. It surfaces — as a
//! separate, ordinary window — only when a person is actually needed: a second
//! factor, a wrong password, anything unexpected, and passkey, where the script
//! fills in the account, gets past that step and then stands aside. (A passkey
//! also needs a real window for the system's own prompt to sit on.)
//!
//! What comes back is not a token. The login is tied to the `pSKey` the window
//! was opened with, so once the portal takes over the page, the caller finishes
//! the same way a QR login does — with `beanfun::complete_login` on the client
//! that minted that key. Reading cookies out of the webview is therefore
//! unnecessary.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use reqwest_cookie_store::CookieStoreMutex;
use std::sync::Arc;
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, Runtime, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::{browser, overlay};

const LABEL_PREFIX: &str = "gamapass-";
/// Short enough that the window keeps up when the main window is dragged.
const POLL_INTERVAL: Duration = Duration::from_millis(80);
/// Long enough to read a mail or open an authenticator app on the way through.
const TIMEOUT: Duration = Duration::from_secs(600);

/// The script asks for a person by putting this in the address; the same poll
/// that watches for the portal picks it up. beanfun's CSP keeps app IPC out of
/// these windows, and this one has no capability anyway.
const NEEDS_USER_FRAGMENT: &str = "kz-gamapass=user";

/// Size of the window once it has to be shown, in CSS pixels.
const WINDOW_SIZE: (f64, f64) = (480.0, 720.0);

/// Hosts that mean the login is done. The sign-in itself wanders off to
/// Gamania's own domains, so anything that is not the login page cannot be the
/// signal — only arriving at the portal can.
const PORTAL_HOSTS: &[&str] = &["tw.beanfun.com", "tw.newlogin.beanfun.com"];

/// A fixed label would collide: tauri only forgets a label once the old
/// window's `Destroyed` event has gone through the event loop.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// What the script should put into Gamania's form.
pub struct Fill {
    pub account: String,
    /// `None` for passkey: the account still goes in and the step is still
    /// advanced — so the user is not made to type it a second time — but the
    /// window is then handed over. A passkey cannot work any other way: the
    /// credential is bound to Gamania's origin, so only their page can ask for
    /// it, and the system's prompt needs that window on screen.
    pub password: Option<String>,
}

/// How a [`wait_for_login`] ended.
pub enum Outcome {
    /// The portal took over the page — the caller should finish the login.
    Completed,
    /// The window was closed, or nothing happened for [`TIMEOUT`].
    Cancelled,
}

/// Open the GamaPass entry point (see `beanfun::go_gamapass`) and wait until
/// the portal takes over.
pub async fn wait_for_login<R: Runtime>(
    app: &AppHandle<R>,
    entry_url: &str,
    jar: &Arc<CookieStoreMutex>,
    fill: Fill,
) -> Result<Outcome, String> {
    let main = app.get_webview_window("main").ok_or("找不到主視窗")?;
    let url: Url = entry_url
        .parse()
        .map_err(|e| format!("登入頁網址錯誤：{e}"))?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("取不到資料夾：{e}"))?
        .join("gamapass-webview");

    cancel(app);
    let label = format!("{LABEL_PREFIX}{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    // 先開空白頁再導過去：cookie 要在第一個真正的請求之前就位，不然 beanfun 綁在
    // 這條 session 上的 nonce 對不起來。
    let blank: Url = "about:blank".parse().map_err(|e| format!("{e}"))?;
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(blank))
        .title("GamaPass 登入")
        .inner_size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .resizable(false)
        .skip_taskbar(true)
        // Hidden while the script drives it; shown the moment a person is needed.
        .visible(false)
        // Its own WebView2 environment, like the captcha window: one user-data
        // folder cannot host two. Reused every time, so nothing piles up.
        .data_directory(data_dir)
        .additional_browser_args(overlay::BROWSER_ARGS)
        .initialization_script(&init_script(&fill))
        .build()
        .map_err(|e| format!("登入視窗開不起來：{e}"))?;

    overlay::disable_tracking_prevention(&window);
    browser::seed_and_navigate(&window, jar, url)?;

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
        let Ok(url) = window.url() else { continue };
        if at_portal(&url) {
            break Outcome::Completed;
        }
        // The script gave up on doing it silently — hand the page over.
        if url.fragment().is_some_and(|f| f.contains(NEEDS_USER_FRAGMENT)) && !window.is_visible().unwrap_or(false) {
            let _ = window.set_skip_taskbar(false);
            show_window(&window, &main);
        }
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

/// The script the window runs on every document it loads. It only acts on
/// Gamania's own host — the credentials must never be handed to a page that
/// merely happens to load in this window.
fn init_script(fill: &Fill) -> String {
    let account = fill.account.as_str();
    let password = fill.password.as_deref().unwrap_or("");
    let json = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    AUTOFILL_JS
        .replace("__ACCOUNT__", &json(account))
        .replace("__PASSWORD__", &json(password))
        .replace("__FRAGMENT__", &json(NEEDS_USER_FRAGMENT))
}

/// Fills Gamania's own form with what the user typed into ours, and submits.
///
/// Fields are found by type and buttons by their label rather than by any
/// selector from their markup: the page is a framework build whose class names
/// are generated, so anything more precise would break on their next deploy.
///
/// Nothing here tries to look like a human or to get past a check. If Gamania
/// asks for a second factor, or anything at all that we did not expect, it asks
/// for the window to be shown and the page is handed to the user as it is.
const AUTOFILL_JS: &str = r##"(() => {
  if (location.hostname !== "accounts.gamania.com") return;
  const ACC = __ACCOUNT__;
  const PW = __PASSWORD__;   // 空的＝passkey：帳號照填，密碼那一步交給使用者
  if (!ACC) return;

  const FRAGMENT = __FRAGMENT__;
  const step = (k) => { try { return sessionStorage.getItem(k); } catch (e) { return null; } };
  const mark = (k) => { try { sessionStorage.setItem(k, "1"); } catch (e) {} };

  // 這個視窗是隱藏的，所以「交給使用者」＝請後端把它顯示出來。改的是 fragment，
  // 不動路徑，對方的路由不會因此跳頁。
  let asked = false;
  const askForUser = () => {
    if (asked) return;
    asked = true;
    try { location.hash = FRAGMENT; } catch (e) {}
  };

  const visible = (el) => el && el.offsetParent !== null && !el.disabled;
  const accountField = () =>
    [...document.querySelectorAll("input")].find(
      (i) => visible(i) && ["text", "tel", "email"].includes((i.type || "").toLowerCase()));
  const passwordField = () =>
    [...document.querySelectorAll("input")].find(
      (i) => visible(i) && (i.type || "").toLowerCase() === "password");

  // 框架綁的是 input 事件，直接指定 value 不會更新它的狀態，送出去會是空的。
  const fill = (el, value) => {
    const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value");
    if (setter && setter.set) setter.set.call(el, value);
    else el.value = value;
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  };

  const clickLabelled = (text) => {
    const hit = [...document.querySelectorAll("button, [role=button]")].find(
      (b) => visible(b) && (b.textContent || "").trim().includes(text));
    if (hit) { hit.click(); return true; }
    return false;
  };

  const started = Date.now();
  const timer = setInterval(() => {
    // 卡住就不要再等了：交給使用者，讓他看到頁面到底停在哪。
    if (Date.now() - started > 20000) { clearInterval(timer); askForUser(); return; }

    if (!step("__kz_acc")) {
      const acc = accountField();
      if (acc && !passwordField()) { fill(acc, ACC); mark("__kz_acc"); }
      return;
    }
    if (!step("__kz_next")) {
      if (passwordField()) { mark("__kz_next"); return; }
      clickLabelled("下一步");
      return;
    }
    // passkey：帳號已經帶進去、也過了這一步，剩下的是使用者的事。
    if (!PW) { clearInterval(timer); askForUser(); return; }
    if (!step("__kz_pw")) {
      const pw = passwordField();
      if (pw) { fill(pw, PW); mark("__kz_pw"); }
      return;
    }
    if (!step("__kz_login")) {
      if (clickLabelled("登入")) mark("__kz_login");
      return;
    }
    // 送出了。再往下一律是人的事：二階段、密碼錯了、或是我們沒想到的畫面。
    // 真的成功的話，頁面會離開這個網域，這支腳本也就不再跑了。
    clearInterval(timer);
    setTimeout(askForUser, 2500);
  }, 300);
})();"##;

/// Show it as an ordinary window, centred on the app — the app sits in the
/// bottom-right corner, and a window that opens across the screen from it reads
/// as something else entirely.
fn show_window<R: Runtime>(window: &WebviewWindow<R>, main: &WebviewWindow<R>) {
    let _ = window.set_size(LogicalSize::new(WINDOW_SIZE.0, WINDOW_SIZE.1));
    if let (Ok(main_pos), Ok(main_size), Ok(size)) =
        (main.outer_position(), main.outer_size(), window.outer_size())
    {
        let x = main_pos.x + (main_size.width as i32 - size.width as i32) / 2;
        let y = main_pos.y + (main_size.height as i32 - size.height as i32) / 2;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    let _ = window.show();
    let _ = window.set_focus();
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

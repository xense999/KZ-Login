//! GamaPass login window.
//!
//! A GamaPass account cannot be signed in over HTTP the way a beanfun account
//! can — the password, and whatever second factor the account carries, belong
//! to Gamania's own page. So this opens that page in a window of its own and
//! lets the user sign in there; we never see the credentials.
//!
//! The window is borderless and pinned over the login page's own content area
//! (see `overlay`), so signing in reads as part of the app rather than as a
//! browser that appeared out of nowhere. In [`Mode::Autofill`] it also starts
//! out covered: the account and password were already typed into our own form,
//! so an injected script puts them into Gamania's fields and submits, and the
//! cover only lifts when a person is actually needed — a second factor, a
//! wrong password, anything unexpected.
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

/// What the window should do once it is open.
pub enum Mode {
    /// Type these into Gamania's form and submit. Empty strings are treated as
    /// [`Mode::Manual`] rather than filling the page with nothing.
    Autofill { account: String, password: String },
    /// Show the page and stay out of the way — passkey, or the user preferring
    /// to type there. A passkey cannot work any other way: the credential is
    /// bound to Gamania's origin, so only their page can ask for it.
    Manual,
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
    region: Region,
    mode: Mode,
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
        .initialization_script(&init_script(&mode))
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

/// The script the window runs on every document it loads. It only acts on
/// Gamania's own host — the credentials must never be handed to a page that
/// merely happens to load in this window.
fn init_script(mode: &Mode) -> String {
    let (account, password) = match mode {
        Mode::Autofill { account, password } if !account.is_empty() && !password.is_empty() => {
            (account.as_str(), password.as_str())
        }
        _ => ("", ""),
    };
    let json = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    AUTOFILL_JS
        .replace("__ACCOUNT__", &json(account))
        .replace("__PASSWORD__", &json(password))
}

/// Fills Gamania's own form with what the user typed into ours, and submits.
///
/// Fields are found by type and buttons by their label rather than by any
/// selector from their markup: the page is a framework build whose class names
/// are generated, so anything more precise would break on their next deploy.
///
/// Nothing here tries to look like a human or to get past a check. If Gamania
/// asks for a second factor, or anything at all that we did not expect, the
/// cover comes off and the page is handed to the user as it is.
const AUTOFILL_JS: &str = r##"(() => {
  if (location.hostname !== "accounts.gamania.com") return;
  const ACC = __ACCOUNT__;
  const PW = __PASSWORD__;
  if (!ACC || !PW) return;

  const COVER_ID = "__kz_cover";
  const DONE = "__kz_autofill_done";
  const step = (k) => { try { return sessionStorage.getItem(k); } catch (e) { return null; } };
  const mark = (k) => { try { sessionStorage.setItem(k, "1"); } catch (e) {} };

  // 蓋住頁面，直到真的需要人接手為止。第一次繪製就要蓋上，否則會閃一下對方的版面。
  const cover = () => {
    if (document.getElementById(COVER_ID) || !document.body) return;
    const el = document.createElement("div");
    el.id = COVER_ID;
    el.setAttribute("style",
      "position:fixed;inset:0;z-index:2147483000;background:#131924;" +
      "display:flex;align-items:center;justify-content:center");
    const spin = document.createElement("div");
    spin.setAttribute("style",
      "width:32px;height:32px;border-radius:50%;border:2px solid rgba(255,255,255,0.07);" +
      "border-top-color:rgba(255,255,255,0.5);animation:__kzspin .8s linear infinite");
    const style = document.createElement("style");
    style.textContent = "@keyframes __kzspin{to{transform:rotate(360deg)}}";
    el.appendChild(spin);
    document.documentElement.appendChild(style);
    document.body.appendChild(el);
  };
  const uncover = () => {
    const el = document.getElementById(COVER_ID);
    if (el) el.remove();
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
    const proto = Object.getPrototypeOf(el);
    const setter = Object.getOwnPropertyDescriptor(proto, "value");
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

  cover();
  const started = Date.now();
  const timer = setInterval(() => {
    cover();
    // 卡住就不要再等了：把畫面交給使用者，讓他看到頁面到底停在哪。
    if (Date.now() - started > 20000) { clearInterval(timer); uncover(); return; }

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
    if (!step("__kz_pw")) {
      const pw = passwordField();
      if (pw) { fill(pw, PW); mark("__kz_pw"); }
      return;
    }
    if (!step(DONE)) {
      if (clickLabelled("登入")) mark(DONE);
      return;
    }
    // 送出了。剩下的一律是人的事：二階段、密碼錯了、或是我們沒想到的畫面。
    clearInterval(timer);
    setTimeout(uncover, 1200);
  }, 300);
})();"##;

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

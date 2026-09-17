//! GamaPass login window.
//!
//! A GamaPass account cannot be signed in over HTTP the way a beanfun account
//! can — the password check, the reCAPTCHA token and the passkey all belong to
//! Gamania's own page and only work on their origin. So the page is loaded in a
//! window of its own; what we control is whether anyone has to look at it.
//!
//! The window starts on **beanfun's** login page and the script presses 「使用
//! gamapass」 there, exactly as a person would. Asking for the entry address
//! ourselves and sending the window straight to Gamania does not work: beanfun
//! ties the OAuth nonce to the session that asked, so the round trip comes back
//! with a nonce it does not recognise (`AUCB001`). Everything — the request for
//! the address, the hop to Gamania, the hop back — has to happen in one browser
//! context.
//!
//! The window is never shown while the script is working: what the user typed
//! into our own form is put into Gamania's fields, and with a password the
//! whole login can finish without their site ever appearing. It surfaces — as a
//! separate, ordinary window — only when a person is actually needed: a second
//! factor, a wrong password, anything unexpected, and passkey, where the script
//! fills in the account, gets past that step and then stands aside. (A passkey
//! also needs a real window for the system's own prompt to sit on.)
//!
//! The login's result stays in that window. Unlike QR — where the sign-in
//! happens on a phone and our own client can still finish on the session key —
//! GamaPass hands `bfWebToken` to the browser that performed it and to nobody
//! else, so the token is read out of the window's cookies and written into our
//! jar. That cookie is also the only sound signal that it worked: the page
//! returns to beanfun whether the sign-in succeeded or failed.

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
/// that watches the window picks it up. beanfun's CSP keeps app IPC out of
/// these windows, and this one has no capability anyway.
const NEEDS_USER_FRAGMENT: &str = "kz-gamapass=user";

/// Size of the window once it has to be shown, in CSS pixels.
const WINDOW_SIZE: (f64, f64) = (480.0, 720.0);

/// Where the finished login leaves its cookies. Read in this order; the token
/// is normally on the portal itself.
const HARVEST_URLS: &[&str] = &[
    "https://tw.beanfun.com/",
    "https://login.beanfun.com/",
    "https://tw.newlogin.beanfun.com/",
];

/// The cookie that *is* the login.
const TOKEN_COOKIE: &str = "bfWebToken";

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
    /// Signed in. Carries every cookie the window ended up with, the token
    /// among them — the caller writes them into its own jar.
    Completed { token: String, cookies: Vec<(String, String)> },
    /// The window was closed, or nothing happened for [`TIMEOUT`].
    Cancelled,
}

/// Open beanfun's login page for `skey` and wait until the portal takes over
/// again with the user signed in.
pub async fn wait_for_login<R: Runtime>(
    app: &AppHandle<R>,
    skey: &str,
    jar: &Arc<CookieStoreMutex>,
    fill: Fill,
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
    // 先開空白頁再導過去：cookie 要在第一個真正的請求之前就位，不然 beanfun 綁在
    // 這條 session 上的 nonce 對不起來。
    let blank: Url = "about:blank".parse().map_err(|e| format!("{e}"))?;
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(blank))
        .title("GamaPass 登入")
        .inner_size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .resizable(false)
        .skip_taskbar(fill.password.is_some())
        // 腳本開得動的時候沒人需要看到這個視窗；要人接手時才現身（見 `show_window`）。
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
    // passkey 一定要人操作，藏起來沒有意義：藏著的話，系統那個 passkey 詢問框會
    // 掛在一個看不見的視窗上跳出來，然後我們的視窗才追上去現身，兩個一起冒出來。
    if fill.password.is_none() {
        show_window(&window, &main);
    }

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
        if let Some(done) = harvest(&window) {
            break done;
        }
        let Ok(url) = window.url() else { continue };
        // The script gave up on doing it silently — hand the page over.
        if url.fragment().is_some_and(|f| f.contains(NEEDS_USER_FRAGMENT)) && !window.is_visible().unwrap_or(false) {
            let _ = window.set_skip_taskbar(false);
            show_window(&window, &main);
        }
    };

    // 只有「使用者自己放棄」才在這裡收掉視窗。判定完成之後還有收尾要做，收尾
    // 失敗時那個畫面就是唯一的線索——先關掉等於把現場清乾淨了。成功的那條路由
    // 呼叫端關（`cancel`）。
    if matches!(outcome, Outcome::Cancelled) {
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.destroy();
        }
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
  const ON_BEANFUN = location.hostname === "login.beanfun.com";
  const ON_GAMANIA = location.hostname === "accounts.gamania.com";
  if (!ON_BEANFUN && !ON_GAMANIA) return;

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

  // offsetParent 對 fixed 定位的元素一律是 null，改看有沒有實際畫出來的方框。
  const visible = (el) => !!el && !el.disabled && el.getClientRects().length > 0;
  const accountField = () =>
    [...document.querySelectorAll("input")].find(
      (i) => visible(i) && ["text", "tel", "email"].includes((i.type || "").toLowerCase()));
  const passwordField = () =>
    [...document.querySelectorAll("input")].find(
      (i) => visible(i) && (i.type || "").toLowerCase() === "password");

  // 直接指定 value，框架的狀態不會跟著動——畫面上看得到字，它內部還當成空的，
  // 於是「下一步」一直是停用的，按了也沒反應。execCommand 走的是真正的輸入路徑，
  // 事件跟使用者打字時長得一樣；不支援時再退回指定 value 並自己發事件。
  const fill = (el, value) => {
    el.focus();
    el.select && el.select();
    let ok = false;
    try { ok = document.execCommand("insertText", false, value); } catch (e) {}
    if (!ok || el.value !== value) {
      const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value");
      if (setter && setter.set) setter.set.call(el, value);
      else el.value = value;
      el.dispatchEvent(new Event("input", { bubbles: true }));
    }
    el.dispatchEvent(new Event("change", { bubbles: true }));
    el.dispatchEvent(new Event("blur", { bubbles: true }));
    return el.value === value;
  };

  // 兩邊的按鈕都不是 <button>：一邊是自訂元件、一邊是框架產生的版面，所以
  // 不限標籤，找「文字對得上、而且自己底下沒有更小的元素也對得上」的那一個
  // ——也就是最貼著文字的那層。點它，事件照樣冒泡到綁著 click 的外層。
  // 停用中的按鈕點了也沒用，而且它停用通常代表「欄位的值它還沒收到」——那時候
  // 該做的是重填，不是一直點。這些頁面用 class 表示停用，不是 disabled 屬性。
  const disabled = (el) => {
    for (let n = el; n && n !== document.body; n = n.parentElement) {
      if (n.disabled || n.getAttribute("aria-disabled") === "true") return true;
      if (/disabled/i.test(n.className || "")) return true;
    }
    return false;
  };

  // 有些元件聽的是按下去那一刻（pointerdown／mousedown），只送 click 不會動。
  const press = (el) => {
    const opts = { bubbles: true, cancelable: true, view: window };
    try { el.dispatchEvent(new PointerEvent("pointerdown", opts)); } catch (e) {}
    el.dispatchEvent(new MouseEvent("mousedown", opts));
    try { el.dispatchEvent(new PointerEvent("pointerup", opts)); } catch (e) {}
    el.dispatchEvent(new MouseEvent("mouseup", opts));
    el.click();
  };

  // 文字對得上的元素可能不只一個（說明文字、彈窗裡的字），所以先挑看起來像按鈕
  // 的那些；都不像才退回「最貼著文字的那一層」。
  const clickLabelled = (text, selector) => {
    const want = text.toLowerCase();
    if (selector) {
      const direct = [...document.querySelectorAll(selector)].filter((el) => visible(el) && !disabled(el));
      if (direct.length === 1) { press(direct[0]); return true; }
    }
    const all = [...document.querySelectorAll("button, [role=button], a, div, span, li, label")]
      .filter((el) => visible(el) && (el.textContent || "").trim().toLowerCase().includes(want))
      .filter((el) => ![...el.children].some(
        (c) => (c.textContent || "").toLowerCase().includes(want)))
      .filter((el) => !disabled(el));
    const looksClickable = (el) =>
      el.tagName === "BUTTON" || el.tagName === "A" || el.getAttribute("role") === "button" ||
      /btn|button/i.test(el.className || "") ||
      getComputedStyle(el).cursor === "pointer";
    const hit = all.find(looksClickable) || all[all.length - 1];
    if (!hit) return false;
    press(hit);
    return true;
  };

  // 視窗平常是藏著的，這行字只有在「交給使用者」時才會被看到——那正是最需要
  // 知道「程式走到哪一步、為什麼停下來」的時候。
  const say = (text) => {
    let tag = document.getElementById("__kz_tag");
    if (!tag) {
      if (!document.body) return;
      tag = document.createElement("div");
      tag.id = "__kz_tag";
      tag.setAttribute("style",
        "position:fixed;left:0;right:0;top:0;z-index:2147483000;padding:6px 10px;" +
        "background:#131924;color:#3dd6c3;font:12px/1.5 system-ui,sans-serif;" +
        "text-align:center;pointer-events:none");
      document.body.appendChild(tag);
    }
    if (tag.textContent !== text) tag.textContent = text;
  };

  const started = Date.now();
  const timer = setInterval(() => {
    // 卡住就不要再等了：交給使用者，讓他看到頁面到底停在哪。
    if (Date.now() - started > 20000) { clearInterval(timer); say("等太久了，交給你"); askForUser(); return; }

    // beanfun 的登入頁：按下它自己的「使用 gamapass」，讓它用自己的 session 去
    // 要跳轉網址。我們代打的話，回程的 nonce 會對不起來。
    if (ON_BEANFUN) {
      // 按下去之後頁面應該就離開這裡了。還在，就是那一下沒生效——重挑一次目標再按。
      // 多按幾次等於多跟它要幾組跳轉網址，所以有上限，到頂就交給使用者。
      const at = Number(step("__kz_goto_at") || 0);
      if (at && Date.now() - at < 3500) { say("已按下使用 gamapass，等它跳轉"); return; }
      const tries = Number(step("__kz_goto_n") || 0);
      if (tries >= 3) { clearInterval(timer); say("按不動「使用 gamapass」，交給你"); askForUser(); return; }
      say(tries ? `再試一次「使用 gamapass」（第 ${tries + 1} 次）` : "找「使用 gamapass」按鈕");
      if (clickLabelled("gamapass", ".use-gama-pass")) {
        try {
          sessionStorage.setItem("__kz_goto_at", String(Date.now()));
          sessionStorage.setItem("__kz_goto_n", String(tries + 1));
        } catch (e) {}
      }
      return;
    }

    if (!step("__kz_acc")) {
      say("填帳號");
      const acc = accountField();
      if (acc && !passwordField() && fill(acc, ACC)) mark("__kz_acc");
      return;
    }
    if (!step("__kz_next")) {
      if (passwordField()) { mark("__kz_next"); return; }
      say("按下一步");
      clickLabelled("下一步");
      return;
    }
    // passkey：帳號已經帶進去、也過了這一步，剩下的是使用者的事。
    if (!PW) { clearInterval(timer); say("帳號已填好，請用 passkey 登入"); askForUser(); return; }
    if (!step("__kz_pw")) {
      say("填密碼");
      const pw = passwordField();
      if (pw && fill(pw, PW)) mark("__kz_pw");
      return;
    }
    if (!step("__kz_login")) {
      say("按登入");
      if (clickLabelled("登入")) mark("__kz_login");
      return;
    }
    // 送出了。再往下一律是人的事：二階段、密碼錯了、或是我們沒想到的畫面。
    // 真的成功的話，頁面會離開這個網域，這支腳本也就不再跑了。
    clearInterval(timer);
    say("已送出，等它回應");
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

/// The token, once the window has it. Everything the beanfun domains hold comes
/// back with it: the session those cookies belong to is the one that just
/// signed in, and our own requests have to look like it from here on.
fn harvest<R: Runtime>(window: &WebviewWindow<R>) -> Option<Outcome> {
    let mut all: Vec<(String, String)> = Vec::new();
    for url in HARVEST_URLS {
        let Ok(cookies) = browser::read_cookies(window, url) else { continue };
        for (name, value) in cookies {
            if !all.iter().any(|(n, _)| n == &name) {
                all.push((name, value));
            }
        }
    }
    let token = all
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(TOKEN_COOKIE))
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())?;
    Some(Outcome::Completed { token, cookies: all })
}

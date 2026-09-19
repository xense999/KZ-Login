//! GamaPass login, carried out inside the app.
//!
//! A GamaPass account cannot be signed in over HTTP the way a beanfun account
//! can — the password check and the reCAPTCHA token belong to Gamania's own
//! page and only work on their origin. So their page is opened in a window the
//! user does not see, and an injected script drives it with what was typed into
//! our form.
//!
//! What makes this worth doing out of sight is that Gamania remembers a device:
//! a *password* login with 保持登入狀態 ticked leaves a session on their side,
//! and the next visit lands on an account list where one click signs in — no
//! password, no second factor. That state lives in this window's cookies, which
//! is why only beanfun's are cleared between logins. A passkey login is never
//! remembered (their page does not send the flag with it), so passkeys are
//! declined here rather than offered.
//!
//! Two things can still need the person. A verification code is asked for in
//! our own page and typed into theirs. Anything else the script does not
//! recognise — an image challenge, a wrong password, a redesign — brings the
//! window out over the login page's own content area, as it is, to be dealt
//! with by hand.
//!
//! The login's result stays in that window. Unlike QR — where the sign-in
//! happens on a phone and our own client can still finish on the session key —
//! GamaPass hands `bfWebToken` to the browser that performed it and to nobody
//! else, so the token is read out of the window's cookies and written into our
//! jar. That cookie is also the only sound signal that it worked: the page
//! returns to beanfun whether the sign-in succeeded or failed.

use reqwest_cookie_store::CookieStoreMutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::browser;
use crate::overlay::{self, Region};

const LABEL_PREFIX: &str = "gamapass-";
/// Short enough that the window keeps up when the main window is dragged.
const POLL_INTERVAL: Duration = Duration::from_millis(80);
/// How often the page is asked where it is and the cookies are read. Far rarer
/// than the poll: each waits on a callback that runs on the main thread, and a
/// login takes seconds at best.
const ASK_EVERY: Duration = Duration::from_millis(600);
/// Long enough to wait for a text message and type it in.
const TIMEOUT: Duration = Duration::from_secs(600);

/// Where the window waits while it is not needed. It has to be a shown window:
/// a hidden one stops painting, the page's transitions never finish, and the
/// dialogs the script is waiting for never open.
const OFFSCREEN: (f64, f64) = (-20000.0, -20000.0);

/// On top of [`overlay::BROWSER_ARGS`]: keep the page running at full speed
/// while it is parked off screen, where Chromium would otherwise count it as
/// covered and throttle it.
const KEEP_AWAKE_ARGS: &str = "--disable-backgrounding-occluded-windows --disable-renderer-backgrounding";
/// Chromium takes one `--disable-features` list, so this one is appended to
/// the list in [`overlay::BROWSER_ARGS`] rather than passed beside it.
const KEEP_AWAKE_FEATURE: &str = "CalculateNativeWinOcclusion";

/// Left over from the previous login, these would let the portal short-cut
/// this one. Gamania's own cookies are deliberately not on the list — they are
/// what makes it remember the device.
const STALE_COOKIE_URLS: &[&str] = &[
    "https://tw.beanfun.com/",
    "https://login.beanfun.com/",
    "https://tw.newlogin.beanfun.com/",
];

/// Where the finished login leaves its cookies. Read in this order; the token
/// is normally on the portal itself.
const HARVEST_URLS: &[&str] = STALE_COOKIE_URLS;

/// The cookie that *is* the login.
const TOKEN_COOKIE: &str = "bfWebToken";

/// A fixed label would collide: tauri only forgets a label once the old
/// window's `Destroyed` event has gone through the event loop.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Bumped by every [`cancel`]. A login holds the value it started under (its
/// [`ticket`]) and gives up once the two differ — which is how a cancel reaches
/// a login that has not opened its window yet.
static CANCELS: AtomicU64 = AtomicU64::new(0);

/// What a login must hold from its very first step, before any network call,
/// so that a cancel during those calls is not lost.
pub fn ticket() -> u64 {
    CANCELS.load(Ordering::Relaxed)
}

/// What the script should put into Gamania's form.
pub struct Fill {
    pub account: String,
    pub password: String,
    /// An account being added rather than one already remembered here. Its
    /// password has never been checked, so the account list is passed over and
    /// the password page is what signs it in — picking the row would let any
    /// password through, and that password is about to be saved.
    pub fresh: bool,
}

/// Where the login stands, as far as the person is concerned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Stage {
    /// The script is getting on with it; nothing to do.
    Working,
    /// Gamania sent a verification code and is waiting for it. `error` is what
    /// their page said about the last one, if it turned it down; `attempt`
    /// counts the codes typed so far, so that being turned down twice with the
    /// same words still reads as a change.
    Code {
        #[serde(default, rename = "sentTo")]
        sent_to: String,
        #[serde(default)]
        error: String,
        #[serde(default)]
        attempt: u32,
    },
    /// The script has let go; the window is out and the page is theirs.
    User,
}

/// How a [`wait_for_login`] ended.
pub enum Outcome {
    /// Signed in. Carries every cookie the window ended up with, the token
    /// among them — the caller writes them into its own jar.
    ///
    /// `password_checked` says Gamania itself accepted [`Fill::password`] on
    /// the way: the script submitted it and was never relieved by the user.
    /// A login through the account list, or one finished by hand, proves
    /// nothing about that password.
    Completed { token: String, cookies: Vec<(String, String)>, password_checked: bool },
    /// Cancelled (see [`cancel`]), or nothing happened for [`TIMEOUT`].
    Cancelled,
}

/// Open beanfun's login page for `skey` and wait until the portal takes over
/// again with the user signed in. `on_stage` hears about every change of
/// [`Stage`]; `region` is where the window goes if it has to come out.
pub async fn wait_for_login<R: Runtime>(
    app: &AppHandle<R>,
    skey: &str,
    jar: &Arc<CookieStoreMutex>,
    fill: Fill,
    region: Region,
    ticket: u64,
    on_stage: impl Fn(&Stage),
) -> Result<Outcome, String> {
    if ticket != self::ticket() {
        return Ok(Outcome::Cancelled);
    }
    let main = app.get_webview_window("main").ok_or("找不到主視窗")?;
    let url: Url = format!("https://login.beanfun.com/Login/Index?pSKey={skey}")
        .parse()
        .map_err(|e| format!("登入頁網址錯誤：{e}"))?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("取不到資料夾：{e}"))?
        .join("gamapass-webview");

    // An older login loses its window, without that counting as a cancel of
    // this one.
    close_windows(app);
    let label = format!("{LABEL_PREFIX}{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));

    // 先開空白頁再導過去：cookie 要在第一個真正的請求之前就位，不然 beanfun 綁在
    // 這條 session 上的 nonce 對不起來。
    let blank: Url = "about:blank".parse().map_err(|e| format!("{e}"))?;
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(blank))
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        // 一開始就以「不搶焦點」的方式顯示在畫面外：事後再 `show()` 會把它啟用，
        // 主視窗的鍵盤焦點就被帶走了。
        .focused(false)
        .position(OFFSCREEN.0, OFFSCREEN.1)
        // 尺寸先照著之後要貼的那一塊：現身時版面才不會當著使用者的面重排一次。
        .inner_size(region.width, region.height)
        .owner(&main)
        .map_err(|e| e.to_string())?
        // Its own WebView2 environment, like the captcha window: one user-data
        // folder cannot host two. Reused every time — that is where Gamania's
        // memory of this device lives.
        .data_directory(data_dir)
        .additional_browser_args(&browser_args())
        .initialization_script(&init_script(&fill))
        .build()
        .map_err(|e| format!("登入視窗開不起來：{e}"))?;

    overlay::disable_tracking_prevention(&window);
    // 資料夾是重用的，上一次登入的 token 可能還在裡面；清除是盡力而為、而且是
    // 非同步的，所以先記下它，之後只認「不是這一顆」的 token。
    let stale_token = read_token(&window, HARVEST_URLS[0]).map(|(token, _)| token);
    browser::seed_and_navigate(&window, jar, STALE_COOKIE_URLS, url)?;

    let started = Instant::now();
    let mut last_ask = Instant::now();
    let mut stage = Stage::Working;
    let mut password_sent = false;
    let outcome = loop {
        if started.elapsed() >= TIMEOUT || ticket != self::ticket() {
            break Outcome::Cancelled;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
        let Some(window) = app.get_webview_window(&label) else {
            break Outcome::Cancelled;
        };
        if last_ask.elapsed() >= ASK_EVERY {
            // 兩個都是同步等主執行緒的回呼（頁面正在換的時候會等到逾時），放到
            // 阻塞用的執行緒去，別佔著 runtime 的 worker。
            let asked = {
                let (window, stale) = (window.clone(), stale_token.clone());
                tauri::async_runtime::spawn_blocking(move || {
                    (harvest(&window, stale.as_deref()), ask(&window))
                })
                .await
            };
            last_ask = Instant::now();
            let Ok((harvested, report)) = asked else { continue };
            if let Some((token, cookies)) = harvested {
                let password_checked = password_sent && stage != Stage::User;
                break Outcome::Completed { token, cookies, password_checked };
            }
            password_sent |= report.as_ref().is_some_and(|r| r.sent);
            // 放手之後就不再收回來：頁面已經交給使用者了，中途又把它藏起來只會讓
            // 他打到一半的東西憑空消失。
            if stage != Stage::User {
                if let Some(now) = report.map(|r| r.stage).filter(|now| *now != stage) {
                    if now == Stage::User {
                        overlay::place(&window, &main, region);
                        let _ = window.set_focus();
                    }
                    stage = now;
                    on_stage(&stage);
                }
            }
        }
        // 每個 tick 都貼一次，主視窗被拖動時才跟得上。
        if stage == Stage::User {
            overlay::place(&window, &main, region);
        }
    };

    // 只有「放棄」才在這裡收掉視窗。判定完成之後還有收尾要做，收尾失敗時那個
    // 畫面就是唯一的線索——先關掉等於把現場清乾淨了。成功的那條路由呼叫端關
    // （`cancel`）。
    // ★只收自己那一顆：這時候可能已經有下一次登入的視窗開著了。
    if matches!(outcome, Outcome::Cancelled) {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.destroy();
        }
    }
    Ok(outcome)
}

/// Type the verification code into Gamania's page. Whether it was right shows
/// up as the next [`Stage`]: on with the login, or `Code` again with an error.
pub fn submit_code<R: Runtime>(app: &AppHandle<R>, code: &str) -> Result<(), String> {
    let digits: String = code.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return Err("請輸入驗證碼".into());
    }
    let window = app
        .webview_windows()
        .into_iter()
        .find_map(|(label, window)| label.starts_with(LABEL_PREFIX).then_some(window))
        .ok_or("登入已經結束了")?;
    window
        .eval(format!("window.__kzCode && window.__kzCode(\"{digits}\")"))
        .map_err(|e| format!("驗證碼送不進去：{e}"))
}

/// Give up the login in progress, wherever it has got to — including one that
/// has not opened its window yet (see [`ticket`]).
pub fn cancel<R: Runtime>(app: &AppHandle<R>) {
    CANCELS.fetch_add(1, Ordering::Relaxed);
    close_windows(app);
}

/// Close the window of a login that is over. Not a cancel: the next login's
/// ticket stays good.
pub fn close_windows<R: Runtime>(app: &AppHandle<R>) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(LABEL_PREFIX) {
            let _ = window.destroy();
        }
    }
}

fn browser_args() -> String {
    let shared = overlay::BROWSER_ARGS.replacen(
        "--disable-features=",
        &format!("--disable-features={KEEP_AWAKE_FEATURE},"),
        1,
    );
    format!("{shared} {KEEP_AWAKE_ARGS}")
}

/// What the script says about itself each time it is asked.
#[derive(Debug, PartialEq, Eq, Deserialize)]
struct Report {
    #[serde(flatten)]
    stage: Stage,
    /// The password has been submitted to Gamania's password page.
    #[serde(default)]
    sent: bool,
}

/// `None` while there is nothing to hear — the page is between documents, or
/// on a host the script stays off.
fn ask<R: Runtime>(window: &WebviewWindow<R>) -> Option<Report> {
    let json = browser::eval_json(window, "window.__kz || null")?;
    serde_json::from_str::<Option<Report>>(&json).ok().flatten()
}

/// The script the window runs on every document it loads. It runs on two hosts:
/// beanfun's login page, where it only presses the GamaPass button, and
/// Gamania's own, where the credentials go. Anywhere else it returns at once —
/// what was typed must never reach a page that merely happens to load here.
fn init_script(fill: &Fill) -> String {
    let json = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    // 一次掃完：分兩次 replace 的話，第二次會掃到第一次剛填進去的內容——帳號裡
    // 只要出現 `__PASSWORD__` 這串字，整段腳本就壞在語法上、而且是無聲的。
    let (account, password) = (json(&fill.account), json(&fill.password));
    AUTOFILL_JS
        .replace("__FRESH__", if fill.fresh { "true" } else { "false" })
        .split("__ACCOUNT__")
        .map(|part| part.replace("__PASSWORD__", &password))
        .collect::<Vec<_>>()
        .join(&account)
}

/// Drives Gamania's pages with what the user typed into ours.
///
/// Every tick it looks at where the page is and does the one thing that page
/// needs, so it does not matter in which order their flow arrives: the account
/// list when the device is remembered, the two-step form when it is not.
///
/// Fields are found by type and buttons by their label rather than by
/// generated class names, which change with every deploy of theirs. The few
/// selectors used are hand-written ones read out of their source
/// (`.use-gama-pass`, `.input-verification-code`, the ARIA roles).
///
/// Nothing here tries to look like a human or to get past a check. Whatever it
/// does not recognise, it reports as `user` and leaves alone.
const AUTOFILL_JS: &str = r##"(() => {
  const ON_BEANFUN = location.hostname === "login.beanfun.com";
  const ON_GAMANIA = location.hostname === "accounts.gamania.com";
  if (!ON_BEANFUN && !ON_GAMANIA) return;

  const ACC = __ACCOUNT__;
  const PW = __PASSWORD__;
  // 新增的帳號：密碼還沒被對方驗過，不走「點一下就進去」那條。
  const FRESH = __FRESH__;
  if (!ACC || !PW) return;

  // passkey 在這裡一律婉拒，等同使用者在系統的框上按取消：用它登入不會被記住，
  // 而且這顆視窗在畫面外，系統的框會憑空冒出來。開了 passkey 優先的帳號會因此
  // 走到對方的替代驗證，那一頁交給使用者。
  try {
    if (navigator.credentials) {
      const refuse = () => Promise.reject(new DOMException("declined", "NotAllowedError"));
      navigator.credentials.get = refuse;
      navigator.credentials.create = refuse;
    }
    if (window.PublicKeyCredential) {
      PublicKeyCredential.isConditionalMediationAvailable = () => Promise.resolve(false);
    }
  } catch (e) {}

  const recall = (k) => { try { return sessionStorage.getItem(k); } catch (e) { return null; } };
  const note = (k, v) => { try { sessionStorage.setItem(k, v); } catch (e) {} };

  // offsetParent 對 fixed 定位的元素一律是 null，改看有沒有實際畫出來的方框。
  const visible = (el) => !!el && !el.disabled && el.getClientRects().length > 0;
  const inputs = (types) => [...document.querySelectorAll("input")].filter(
    (i) => visible(i) && types.includes((i.type || "").toLowerCase()));
  const passwordField = () => inputs(["password"])[0];
  const accountField = () => inputs(["text", "tel", "email"])[0];
  const codeBoxes = () => [...document.querySelectorAll(".input-verification-code input")].filter(visible);

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
    // 對方可能會重排格式（去空白、補國碼），所以只看有沒有收下，不比對全文。
    return el.value !== "";
  };

  // 停用中的按鈕點了也沒用，而且它停用通常代表「欄位的值它還沒收到」——那時候
  // 該做的是重填，不是一直點。這些頁面用 class 表示停用，不是 disabled 屬性。
  const disabled = (el) => {
    for (let n = el; n && n !== document.body; n = n.parentElement) {
      if (n.disabled || n.getAttribute("aria-disabled") === "true") return true;
      if (/(^|\s)(is-)?disabled(\s|$)/i.test(n.className || "")) return true;
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

  // 兩邊的按鈕都不見得是 <button>，所以不限標籤，找「文字對得上、而且自己底下
  // 沒有更小的元素也對得上」的那一層——最貼著文字的那層。點它，事件照樣冒泡到
  // 綁著 click 的外層。`exact` 是給短標籤用的：「登入」兩個字到處都是。
  const labelled = (text, exact) => {
    const want = text.toLowerCase();
    const hit = (el) => {
      const t = (el.textContent || "").trim().toLowerCase();
      return exact ? t === want : t.includes(want);
    };
    const all = [...document.querySelectorAll("button, [role=button], a, div, span, li, label")]
      .filter((el) => visible(el) && hit(el))
      .filter((el) => ![...el.children].some(hit))
      .filter((el) => !disabled(el));
    const looksClickable = (el) =>
      el.tagName === "BUTTON" || el.tagName === "A" || el.getAttribute("role") === "button" ||
      /btn|button/i.test(el.className || "") ||
      getComputedStyle(el).cursor === "pointer";
    return all.find(looksClickable) || all[all.length - 1] || null;
  };
  // 同一顆按鈕不連按：一次沒反應多半是頁面還在忙，連按等於多送幾次請求。
  const pressOnce = (key, el, every) => {
    if (!el) return false;
    const at = Number(recall(key) || 0);
    if (at && Date.now() - at < (every || 2500)) return true;
    note(key, String(Date.now()));
    press(el);
    return true;
  };

  // 現身之後使用者看得到這一行，所以直接告訴他為什麼輪到他。
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

  const report = (stage, extra) => {
    window.__kz = Object.assign({ stage, sent: !!recall("__kz_sent") }, extra || {});
  };
  report("working");

  // 放手：之後這一頁是使用者的。passkey 的推銷照樣替他婉拒——那是唯一一個
  // 按錯了就回不來的地方。
  let handedOver = false;
  const handOver = (why) => { handedOver = true; say(why); report("user"); };

  // 登入成功後對方會問要不要改用 passkey。答應了，這個帳號以後就只走 passkey，
  // 再也到不了密碼那一頁，也就再也不會被記住。
  const declinePasskey = () => {
    const keep = labelled("繼續使用密碼", true);
    if (keep) return pressOnce("__kz_keep_pw", keep);
    const later = labelled("稍後再說", true);
    if (!later) return false;
    for (let n = later.parentElement; n && n !== document.body; n = n.parentElement) {
      if (/passkey/i.test(n.textContent || "")) return pressOnce("__kz_later", later);
    }
    return false;
  };

  // 清單上的帳號是遮起來的（`+886 922 *** *93`）。把星號當成缺口：剩下的幾段
  // 依序對得上帳號的頭跟尾，就是它。手機號碼對方會換成國碼開頭。
  const forms = (() => {
    const a = ACC.trim().toLowerCase();
    const out = [a];
    if (/^09\d{8}$/.test(a)) out.push("+886" + a.slice(1));
    return out;
  })();
  const matchesMask = (shown) => {
    const mask = shown.replace(/\s+/g, "").toLowerCase();
    const parts = mask.split(/\*+/).filter(Boolean);
    if (!parts.length) return false;
    return forms.some((form) => {
      // 頭尾沒被遮住的那一端才要求貼齊。
      if (!mask.startsWith("*") && !form.startsWith(parts[0])) return false;
      if (!mask.endsWith("*") && !form.endsWith(parts[parts.length - 1])) return false;
      let from = 0;
      for (const part of parts) {
        const at = form.indexOf(part, from);
        if (at < 0) return false;
        from = at + part.length;
      }
      return true;
    });
  };
  const rememberedRow = () => {
    const rows = [...document.querySelectorAll("#main [role=button]")].filter(visible).filter((row) =>
      [...row.querySelectorAll("*")].some((el) =>
        !el.children.length && (el.textContent || "").includes("*") && matchesMask(el.textContent)));
    // 兩列都對得上就不猜：寧可走一次密碼，也不要登進別人的帳號。
    return rows.length === 1 ? rows[0] : null;
  };

  // 圖形驗證的框是 reCAPTCHA 自己排的（約 400×580），常被擺到畫面外；把它固定在
  // 視窗正中並等比縮到放得進去（同 captcha 視窗的做法）。
  const challenge = () => {
    const frame = document.querySelector("iframe[src*='recaptcha'][src*='bframe']");
    return frame && frame.offsetWidth && frame.offsetHeight ? frame : null;
  };
  const fitChallenge = (frame) => {
    const w = frame.offsetWidth, h = frame.offsetHeight, m = 6;
    const vw = window.innerWidth, vh = window.innerHeight;
    const k = Math.min(1, (vw - m * 2) / w, (vh - m * 2) / h);
    const set = (prop, value) => frame.style.setProperty(prop, value, "important");
    set("position", "fixed");
    set("left", Math.round((vw - w * k) / 2) + "px");
    set("top", Math.round((vh - h * k) / 2) + "px");
    set("transform", "scale(" + k + ")");
    set("transform-origin", "0 0");
  };

  // 驗證碼由我們的頁面問，問到了從這裡打進去。第一格收到整串數字時，對方的
  // 元件會自己分到四格裡。
  window.__kzCode = (code) => {
    const box = codeBoxes()[0];
    if (!box) return;
    note("__kz_code_at", String(Date.now()));
    note("__kz_code_n", String(Number(recall("__kz_code_n") || 0) + 1));
    fill(box, code);
  };
  const codeError = () => {
    const el = document.querySelector(".input-verification-code .color-red");
    return el ? (el.textContent || "").trim() : "";
  };
  const codeSentTo = () => {
    const el = [...document.querySelectorAll("#main div, #header div")].find((d) =>
      visible(d) && !d.children.length && /^\+?[\d\s*]{6,}$|@/.test((d.textContent || "").trim()));
    return el ? el.textContent.trim() : "";
  };

  // 同一頁待太久就是卡住了：可能是密碼錯、可能是沒見過的畫面。
  let where = "", since = Date.now();
  const stuck = (key, limit) => {
    if (key !== where) { where = key; since = Date.now(); }
    return Date.now() - since > limit;
  };

  setInterval(() => {
    const frame = ON_GAMANIA && challenge();
    if (frame) fitChallenge(frame);
    if (ON_GAMANIA && declinePasskey()) return;
    if (handedOver) return;
    if (frame) return handOver("請完成圖形驗證");

    const path = location.pathname.replace(/\/+$/, "").toLowerCase();

    // beanfun 的登入頁：按下它自己的「使用 gamapass」，讓它用自己的 session 去
    // 要跳轉網址。我們代打的話，回程的 nonce 會對不起來。
    if (ON_BEANFUN) {
      if (path !== "/login/index") {
        // 登完繞回來會經過這個網域；停在這裡不走，就是 beanfun 不收這次登入。
        if (stuck("bf:" + path, 8000)) handOver("beanfun 沒有完成這次登入");
        return;
      }
      // 多按幾次等於多跟它要幾組跳轉網址，所以有上限，到頂就交給使用者。
      const tries = Number(recall("__kz_goto_n") || 0);
      const at = Number(recall("__kz_goto_at") || 0);
      if (at && Date.now() - at < 3500) return;
      if (tries >= 3) return handOver("按不動「使用 gamapass」，請你自己點");
      const go = document.querySelector(".use-gama-pass") || labelled("gamapass");
      if (go && visible(go)) {
        note("__kz_goto_at", String(Date.now()));
        note("__kz_goto_n", String(tries + 1));
        press(go);
      }
      return;
    }

    // 對方跳了一個要人按「確定」的框：密碼錯、驗證碼過期、帳號被鎖、或別的我們
    // 沒料到的話。不替使用者按掉——那些字是寫給他看的。選帳號頁的「已過期」是
    // 例外，那個我們知道怎麼接。
    const notice = [...document.querySelectorAll("[role=dialog], .el-message-box")].find(visible);
    if (notice && path !== "/login/select-account" && labelled("確定", true)) {
      return handOver("請看畫面上的訊息");
    }

    // 要驗證碼：哪一頁問的都一樣處理。剛送出的那幾秒不回報，免得舊的錯誤訊息
    // 還掛在畫面上就被當成這一次的結果。
    if (codeBoxes().length) {
      if (Date.now() - Number(recall("__kz_code_at") || 0) < 2500) return;
      return report("code", {
        sentTo: codeSentTo(), error: codeError(), attempt: Number(recall("__kz_code_n") || 0),
      });
    }
    report("working");

    if (path === "/login/select-account") {
      // 記住的那一筆過期了，對方會跳一個框說要重新登入；按掉它就回到登入頁。
      if (/已過期/.test(document.body.textContent || "")) {
        const ok = labelled("確定", true);
        if (ok) { note("__kz_expired", "1"); return void pressOnce("__kz_expired_ok", ok); }
      }
      const row = FRESH || recall("__kz_expired") ? null : rememberedRow();
      if (row) { pressOnce("__kz_row", row, 6000); }
      else if (document.querySelector("#main [role=button]")) {
        pressOnce("__kz_other", labelled("使用其他帳號登入"));
      }
      if (stuck(path, 20000)) handOver("選不到帳號，請你自己點");
      return;
    }

    if (path === "/login") {
      // 先看卡住了沒：下面每一步做不成都是直接 return，輪不到後面的檢查。
      if (stuck(path, 15000)) {
        return handOver(recall("__kz_next")
          ? "這個帳號可能開了 passkey 優先：請改用驗證碼登入，之後到會員中心關掉它"
          : "這一步請你自己來");
      }
      const acc = accountField();
      if (acc && (!recall("__kz_acc") || !acc.value)) {
        if (fill(acc, ACC)) note("__kz_acc", "1");
        return;
      }
      // 沒勾這個，對方就不會記住這台裝置，下次又得從頭驗一遍。
      const keep = [...document.querySelectorAll("[role=checkbox]")].find(
        (el) => visible(el) && /保持登入/.test(el.textContent || ""));
      // 不連按：狀態晚一拍才反映的話，連按會把它又切回去。
      if (keep && keep.getAttribute("aria-checked") !== "true") return void pressOnce("__kz_keep", keep, 1200);
      if (acc) pressOnce("__kz_next", labelled("下一步", true));
      return;
    }

    if (path === "/login/input-password") {
      const pw = passwordField();
      if (!recall("__kz_sent")) {
        if (pw && fill(pw, PW)) {
          const go = labelled("登入", true);
          if (go) { note("__kz_sent", "1"); press(go); }
        }
      }
      // 送出後還留在這一頁：密碼不對，或是對方有話要說。
      if (stuck(path, 10000)) handOver("登入沒有成功，請看畫面上的訊息");
      return;
    }

    // 其餘的頁面（授權中轉、登入完成的過場）只是路過。停太久才是有事。
    if (stuck(path, 15000)) handOver("這一步請你自己來");
  }, 300);
})();"##;

/// The token, once the window has it. Everything the beanfun domains hold comes
/// back with it: the session those cookies belong to is the one that just
/// signed in, and our own requests have to look like it from here on.
///
/// `stale` is the token the reused folder already held when this login began;
/// it is not this login's, however long it lingers.
fn harvest<R: Runtime>(
    window: &WebviewWindow<R>,
    stale: Option<&str>,
) -> Option<(String, Vec<(String, String)>)> {
    // 讀 cookie 是同步等一個跑在主執行緒的回呼，所以第一個網域沒有 token 就先
    // 收手——token 幾乎都在 portal 上，其餘兩個只在真的成功時才需要一起帶走。
    let (token, first) = read_token(window, HARVEST_URLS[0])?;
    if stale == Some(token.as_str()) {
        return None;
    }

    let mut all = first;
    for url in &HARVEST_URLS[1..] {
        let Ok(cookies) = browser::read_cookies(window, url) else { continue };
        for (name, value) in cookies {
            if !all.iter().any(|(n, _)| n == &name) {
                all.push((name, value));
            }
        }
    }
    Some((token, all))
}

/// The token among the cookies of `url`, and those cookies.
fn read_token<R: Runtime>(
    window: &WebviewWindow<R>,
    url: &str,
) -> Option<(String, Vec<(String, String)>)> {
    let cookies = browser::read_cookies(window, url).ok()?;
    let token = cookies
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(TOKEN_COOKIE))
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())?;
    Some((token, cookies))
}

#[cfg(test)]
mod tests {
    use super::{browser_args, init_script, Fill, Report, Stage};

    #[test]
    fn reads_what_the_script_reports() {
        let parse = |s: &str| serde_json::from_str::<Option<Report>>(s).unwrap();
        assert_eq!(parse("null"), None);
        assert_eq!(parse(r#"{"stage":"working"}"#), Some(Report { stage: Stage::Working, sent: false }));
        assert_eq!(parse(r#"{"stage":"user","sent":true}"#), Some(Report { stage: Stage::User, sent: true }));
        assert_eq!(
            parse(r#"{"stage":"code","sent":true,"sentTo":"+886 922 313 293","error":"","attempt":1}"#),
            Some(Report {
                stage: Stage::Code { sent_to: "+886 922 313 293".into(), error: String::new(), attempt: 1 },
                sent: true,
            })
        );
    }

    /// 帳號裡出現佔位字串時，第二次替換不可以掃到第一次填進去的內容。
    #[test]
    fn placeholders_in_the_account_stay_literal() {
        let js = init_script(&Fill { account: "__PASSWORD__".into(), password: "pw".into(), fresh: true });
        assert!(js.contains("const FRESH = true;"));
        assert!(js.contains(r#"const ACC = "__PASSWORD__";"#));
        assert!(js.contains(r#"const PW = "pw";"#));
    }

    /// Chromium 只認最後一個 `--disable-features`，所以只能有一個。
    #[test]
    fn keeps_a_single_feature_list() {
        let args = browser_args();
        assert_eq!(args.matches("--disable-features=").count(), 1);
        assert!(args.contains("CalculateNativeWinOcclusion,"));
    }
}

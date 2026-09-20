//! 帳號瀏覽器：帶著某個 beanfun 帳號登入態、有分頁的內建瀏覽器。
//!
//! 架構＝雙視窗貼合：工具列視窗（label `browser-shell-<組號>`，自己的 webview 畫
//! 邊框＋標題列＋分頁＋導覽列）是 owner；每個分頁是一個獨立的 `WebviewWindow`
//! （owned window，疊在工具列視窗畫出的框裡面），切分頁＝顯示/隱藏。
//!
//! **為什麼不用 multi-webview（`Window::add_child`）**：子 webview 在 Windows 上
//! 收不到鍵盤輸入（Chromium 視自己為未啟用而丟鍵，2026-08-18 診斷坐實，見
//! docs/瀏覽器重寫目標.md）。獨立視窗的 webview 與主視窗同構，鍵盤天生正常。
//!
//! 契約見 `docs/規範.md` 的 `browser` 條目。

use reqwest_cookie_store::CookieStoreMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{
    webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent},
    AppHandle, Emitter, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

/// 工具列視窗 label 的前綴，後面接視窗組的號碼（`browser-shell-3`）。
/// 同時出現在 `capabilities/browser.json`（glob `browser-shell-*`）。
///
/// ★**為什麼不是固定的 `browser`**：`destroy()` 是丟給主執行緒的訊息
/// （tauri-runtime-wry 的 `destroy()` 一律走 `proxy.send_event`，連本來就在主
/// 執行緒也不例外），呼叫回來時 tauri 的 webview 簿記還留著那個 label——簿記要
/// 等 event loop 收到 `Destroyed` 才清。所以「砍掉幽靈→馬上用同一個 label 建新
/// 視窗」**必定**撞上 `a webview with label "browser" already exists`，
/// 而 Edge 在背景更新收掉 WebView2 之後走的正是這條路：使用者從此開不了瀏覽器，
/// 要重開整個 app 才會好。換號碼就完全不必等簿記清乾淨。
const TOOLBAR_LABEL_PREFIX: &str = "browser-shell-";

/// 第 `generation` 組的工具列 label。號碼來源就是 `BrowserState::generation`。
fn toolbar_label(generation: u64) -> String {
    format!("{TOOLBAR_LABEL_PREFIX}{generation}")
}

/// 現在這一組的工具列視窗。舊組的 label 號碼不同，查不到就是沒開著。
fn toolbar_window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    let generation = STATE.lock().ok()?.generation;
    app.get_webview_window(&toolbar_label(generation))
}

/// 推事件給現在這一組的工具列。
fn toolbar_target() -> tauri::EventTarget {
    let generation = STATE.lock().map(|s| s.generation).unwrap_or(0);
    tauri::EventTarget::webview_window(toolbar_label(generation))
}

/// 分頁視窗 label 的前綴。掃殘骸時靠它認人，不必先問得到 id。
/// 分頁視窗不在任何 capability 檔裡＝零 IPC。
const TAB_LABEL_PREFIX: &str = "browser-tab-";

fn tab_label(id: u64) -> String {
    format!("{TAB_LABEL_PREFIX}{id}")
}

/// CSS 像素換算成實體像素的比率。**不等於 `scale_factor()`**：Windows 的
/// 「協助工具 → 文字大小」不進 tao 回報的 DPI，卻會被 WebView2 併進自己的縮放
/// （見 `crate::win::text_scale_factor`），所以殼層畫出來的標題列在螢幕上比
/// `TOOLBAR_H * scale_factor()` 高——少算就會讓分頁視窗往上蓋掉分頁列與網址列。
fn text_scale() -> f64 {
    #[cfg(windows)]
    {
        crate::win::text_scale_factor()
    }
    #[cfg(not(windows))]
    {
        1.0
    }
}

fn css_to_px(window_scale: f64) -> f64 {
    window_scale * text_scale()
}

/// 這個 label 是不是帳號瀏覽器的視窗（任何一組的工具列或分頁）。掃殘骸用。
fn is_browser_label(label: &str) -> bool {
    label.starts_with(TOOLBAR_LABEL_PREFIX) || label.starts_with(TAB_LABEL_PREFIX)
}

/// 標題列與導覽列的高度。
const TITLEBAR_H: f64 = 42.0;
const NAVBAR_H: f64 = 42.0;
/// 工具列總高（從視窗頂端到分頁內容頂端）。
const TOOLBAR_H: f64 = TITLEBAR_H + NAVBAR_H;
/// 邊框粗細。這圈邊框同時是左／右／下三邊唯一抓得到的縮放區：Windows 上
/// `decorations(false)` 的視窗沒有原生 resize 邊框，分頁視窗又蓋住框內整塊，
/// 滑鼠只碰得到工具列視窗露出來的這一圈。
const EDGE: f64 = 3.0;

const DEFAULT_W: f64 = 1230.0;
const DEFAULT_H: f64 = 720.0;
const MIN_W: f64 = 720.0;
const MIN_H: f64 = 480.0;

/// beanfun 的登入態散在這三個網址底下。要清舊的、要撈新的，都是問這三個。
pub(crate) const SESSION_COOKIE_URLS: &[&str] = &[
    "https://tw.beanfun.com/",
    "https://login.beanfun.com/",
    "https://tw.newlogin.beanfun.com/",
];

/// 給 `GetCookies` 空網址＝儲存區裡每一顆，不分網域。換帳號整個清空時用。
const EVERY_COOKIE: &[&str] = &[""];

/// 瀏覽器開啟後與「+」新分頁的起始頁。
const HOME_URL: &str = "https://tw.beanfun.com/";

/// 把 WebView2 預設那條粗滾動條換成細的。純外觀，注進網頁只動 scrollbar 偽元素。
/// 顏色配網頁而不是配 app 主題——它是網頁的滾動條，不該跟著 app 深淺色跑。
/// 初始化腳本在文件解析前就跑，所以 `document.head` 可能還不存在，兩個時機都補一次。
const THIN_SCROLLBAR_SCRIPT: &str = r#"
(function () {
  var css = "html{scrollbar-width:thin;scrollbar-color:rgba(0,0,0,0.3) transparent}"
    + "::-webkit-scrollbar{width:7px;height:7px}"
    + "::-webkit-scrollbar-track{background:transparent}"
    + "::-webkit-scrollbar-thumb{background:rgba(0,0,0,0.28);border-radius:4px}"
    + "::-webkit-scrollbar-thumb:hover{background:rgba(0,0,0,0.45)}"
    + "::-webkit-scrollbar-corner{background:transparent}";
  function apply() {
    if (document.getElementById("kz-thin-scrollbar")) return;
    var parent = document.head || document.documentElement;
    if (!parent) return;
    var style = document.createElement("style");
    style.id = "kz-thin-scrollbar";
    style.textContent = css;
    parent.appendChild(style);
  }
  apply();
  document.addEventListener("DOMContentLoaded", apply);
})();
"#;

// ─── Cookie 取出與篩選 ────────────────────────────────────────────────────────

/// 一顆準備注入 webview 的 cookie。
#[derive(Debug, Clone, PartialEq, Eq)]
struct WebviewCookie {
    name: String,
    value: String,
    /// 已正規化：跨子網域的帶前導點（`.beanfun.com`），host-only 的不帶。
    domain: String,
    path: String,
    secure: bool,
    http_only: bool,
}

/// cookie 的網域歸屬：`Host` 只認這台主機，`Suffix` 連子網域一起認。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DomainKind {
    Host,
    Suffix,
}

/// 從 jar 讀出來、還沒篩選過的一顆 cookie。
#[derive(Debug, Clone, PartialEq, Eq)]
struct JarCookie {
    name: String,
    value: String,
    domain: String,
    kind: DomainKind,
    path: String,
    secure: bool,
    http_only: bool,
}

/// 只有這個網域底下的 cookie 有登入意義，用來判斷「這個 session 還帶不帶登入態」。
/// **不是注入的過濾條件**——見 `select_for_injection`。
const BEANFUN_DOMAIN: &str = "beanfun.com";
const BEANFUN_SUBDOMAIN_SUFFIX: &str = ".beanfun.com";

/// 注入的 cookie 給多久的有效期。給了到期時間才是 persistent cookie——session
/// cookie 只活在建立它的那顆 webview 裡，其他分頁與網頁彈窗看不到。
const INJECTED_COOKIE_TTL_SECS: f64 = 6.0 * 60.0 * 60.0;

/// 這個網域是不是 beanfun 站台（本身或其子網域）。
fn is_beanfun_domain(domain: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    d == BEANFUN_DOMAIN || d.ends_with(BEANFUN_SUBDOMAIN_SUFFIX)
}

/// 把 jar 裡**每一顆** cookie 都轉成待注入的形式，並把網域正規化成 WebView2
/// cookie manager 的形式：跨子網域的帶前導點、host-only 的不帶。前導點的有無決定
/// cookie 會不會被送到 `tw.beanfun.com` 以外的子網域，所以不能一律加或一律去。
///
/// **刻意不按網域過濾**。原本只注 `beanfun.com` 底下的，結果網頁點出去的彈窗
/// （會走 gamania 的 SSO 或其他關聯網域）沒有登入態——被丟掉的正是那些 cookie。
/// jar 裡的東西全都是登入流程中由伺服器發下來的，整批注入就等於「這顆 webview
/// 自己登入過一次」，比我們自己挑要注哪些忠實得多。
fn select_for_injection(jar: &[JarCookie]) -> Vec<WebviewCookie> {
    jar.iter()
        .map(|c| {
            let bare = c.domain.trim_start_matches('.');
            WebviewCookie {
                name: c.name.clone(),
                value: c.value.clone(),
                domain: match c.kind {
                    DomainKind::Host => bare.to_string(),
                    DomainKind::Suffix => format!(".{bare}"),
                },
                path: c.path.clone(),
                secure: c.secure,
                http_only: c.http_only && !is_js_readable_cookie(&c.name),
            }
        })
        .collect()
}

/// 這顆 cookie 必須讓網頁的 JS 用 `document.cookie` 讀得到，注入時不能帶 HttpOnly。
///
/// `tw.newlogin.beanfun.com/checkin_step2.aspx`（活動頁與網頁版遊戲啟動的 SSO 檢查
/// 點）裡的 `DealWebToken()` 是這樣認登入的：
///
/// ```js
/// var strWebToken = readCookie("bfWebToken");   // = document.cookie
/// if (strWebToken == null || strWebToken.length < 30) { GotoLoginPage(); }
/// ```
///
/// 我們的 jar 收到的 `bfWebToken` 帶著 HttpOnly，照抄注進去 JS 就讀不到，SSO 一律
/// 被 `GotoLoginPage()` 打回 `login.beanfun.com` 的掃碼登入頁。真瀏覽器登入後這顆
/// 讀得到（否則官方自己的活動頁 SSO 也會壞），所以拿掉 HttpOnly 才是還原真實登入
/// 態，不是放寬。HttpOnly 只管 JS 能不能讀，送不送出這顆 cookie 與它無關。
fn is_js_readable_cookie(name: &str) -> bool {
    name.eq_ignore_ascii_case("bfWebToken")
}

/// 把 cookie store 的內容讀成中間形式。網域既非 HostOnly 也非 Suffix 的（jar 裡
/// 尚未綁定主機的項目）沒有可用的注入網域，直接跳過。
fn read_jar(store: &cookie_store::CookieStore) -> Vec<JarCookie> {
    store
        .iter_unexpired()
        .filter_map(|c| {
            let (domain, kind) = match &c.domain {
                cookie_store::CookieDomain::HostOnly(h) => (h.clone(), DomainKind::Host),
                cookie_store::CookieDomain::Suffix(s) => (s.clone(), DomainKind::Suffix),
                _ => return None,
            };
            Some(JarCookie {
                name: c.name().to_string(),
                value: c.value().to_string(),
                domain,
                kind,
                path: c.path.to_string(),
                secure: c.secure().unwrap_or(false),
                http_only: c.http_only().unwrap_or(false),
            })
        })
        .collect()
}

/// 把登入 session 的 cookie jar 讀成待注入清單。
fn cookies_from_jar(jar: &Arc<CookieStoreMutex>) -> Vec<WebviewCookie> {
    let Ok(store) = jar.lock() else {
        return Vec::new();
    };
    select_for_injection(&read_jar(&store))
}

// ─── 分頁狀態 ─────────────────────────────────────────────────────────────────

/// 一個分頁。視窗本體用 `tab_label(id)` 從 app 查，這裡只放要推給殼層的資料。
#[derive(Debug, Clone)]
struct Tab {
    id: u64,
    title: String,
    url: String,
}

/// 瀏覽器的可變狀態。全 app 同時只有一組瀏覽器（見 `WINDOW_OWNER`），所以是單一
/// 靜態。cookies 是 `open()` 時從 jar 讀出的快照——「+」開新分頁時要重新注入，
/// 而 command 端沒有 token 可以回頭解 jar。
struct BrowserState {
    tabs: Vec<Tab>,
    active: u64,
    next_id: u64,
    cookies: Vec<WebviewCookie>,
    /// 這一組視窗的第一個分頁注入之前，要怎麼整理 cookie 儲存區。第一個分頁領走
    /// 之後就是 `Keep`：之後的「+」分頁只注入、不刪——瀏覽期間網站自己發的同名
    /// cookie（別的子網域的 `ASP.NET_SessionId` 之類）是使用者正在走的流程，刪了
    /// 就把那個流程的 session 砍斷了。
    tidy: Tidy,
    /// 哪一組視窗擁有現在這份狀態。`destroy()` 是排進主執行緒的，舊視窗的
    /// `Destroyed` 可能在下一組都建好之後才跑到——它一律清空 STATE 與
    /// `WINDOW_OWNER`，那就會把新視窗的 cookie 清掉（開出來直接是未登入，
    /// 而且沒有任何錯誤訊息）。每組視窗記住自己的號碼，號碼不對就不要動。
    generation: u64,
}

/// 注入之前對 cookie 儲存區做的事。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tidy {
    /// 什麼都不刪。
    Keep,
    /// 同一個帳號再開一次：只刪 beanfun 登入網址底下與注入批次**同名**的。
    SameNames,
    /// 換了帳號：整個清空。清成功了，儲存區才算是這個帳號的。
    Everything { account: String },
}

static STATE: Mutex<BrowserState> = Mutex::new(BrowserState {
    tabs: Vec::new(),
    active: 0,
    next_id: 1,
    cookies: Vec::new(),
    tidy: Tidy::Keep,
    generation: 0,
});

/// 推給殼層的分頁列狀態。
const TABS_EVENT: &str = "browser://tabs";

#[derive(Clone, serde::Serialize)]
struct TabEntry {
    id: u64,
    title: String,
    active: bool,
}

fn emit_tabs<R: Runtime>(app: &AppHandle<R>) {
    let entries: Vec<TabEntry> = {
        let Ok(state) = STATE.lock() else { return };
        state
            .tabs
            .iter()
            .map(|t| TabEntry {
                id: t.id,
                title: t.title.clone(),
                active: t.id == state.active,
            })
            .collect()
    };
    let _ = app.emit_to(toolbar_target(), TABS_EVENT, entries);
}

/// 關掉一個分頁後該輪到誰。優先右邊鄰居、沒有就左邊；關的不是作用中分頁則不變。
fn next_active_after_close(ids: &[u64], closed: u64, active: u64) -> Option<u64> {
    if closed != active {
        return ids.iter().copied().find(|&i| i != closed).map(|_| active);
    }
    let pos = ids.iter().position(|&i| i == closed)?;
    ids.get(pos + 1)
        .or_else(|| pos.checked_sub(1).and_then(|p| ids.get(p)))
        .copied()
}

// ─── 導覽 ─────────────────────────────────────────────────────────────────────

/// 作用中分頁的網址，推給殼層的網址列。每次頁面載入完或切分頁就推一份。
const NAV_EVENT: &str = "browser://nav";

#[derive(Clone, serde::Serialize)]
struct NavState {
    url: String,
}

fn emit_nav<R: Runtime>(app: &AppHandle<R>, url: &str) {
    let _ = app.emit_to(toolbar_target(), NAV_EVENT, NavState { url: url.to_string() });
}

/// 把使用者在網址列打的東西變成可導覽的網址。看得出是網址就直接開（裸主機補
/// https）；看不出來的一律丟給 Google 搜尋。**永遠只回 http(s)**——`file:`、
/// `javascript:` 這類輸入會變成「搜尋那串字」而不是被執行，網址列因此仍然
/// 不是任意 scheme 的入口。
fn normalize_url(input: &str) -> Result<Url, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("請輸入網址或搜尋字詞".to_string());
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        if let Ok(url) = raw.parse::<Url>() {
            if url.scheme() == "http" || url.scheme() == "https" {
                return Ok(url);
            }
        }
        // 有 http 前綴卻 parse 不過（例如「https://1.5」）＝其實不是網址
        return Ok(search_url(raw));
    }
    // 看起來像裸主機：有點號、沒空白、沒有會被當成 scheme 的冒號
    if raw.contains('.') && !raw.contains(' ') && !raw.contains(':') {
        if let Ok(url) = format!("https://{raw}").parse::<Url>() {
            return Ok(url);
        }
    }
    Ok(search_url(raw))
}

/// 把搜尋字詞包成 Google 查詢網址。`query_pairs_mut` 會處理百分比編碼，
/// 中文與特殊字元都安全。
fn search_url(query: &str) -> Url {
    let mut url: Url = "https://www.google.com/search"
        .parse()
        .expect("google search base url is valid");
    url.query_pairs_mut().append_pair("q", query);
    url
}

/// WebView2 的上一頁／下一頁。Tauri 2.11 只包了 `navigate` 與 `reload`，這兩個得
/// 自己走 COM（wry 0.56 有包，但 tauri 尚未曝露）。
#[cfg(windows)]
fn go_history<R: Runtime>(tab: &WebviewWindow<R>, back: bool) -> Result<(), String> {
    tab.with_webview(move |platform| unsafe {
        if let Ok(core) = platform.controller().CoreWebView2() {
            let _ = if back { core.GoBack() } else { core.GoForward() };
        }
    })
    .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn go_history<R: Runtime>(_tab: &WebviewWindow<R>, _back: bool) -> Result<(), String> {
    Err("只支援 Windows".to_string())
}

fn active_tab<R: Runtime>(app: &AppHandle<R>) -> Result<WebviewWindow<R>, String> {
    let id = {
        let state = STATE.lock().map_err(|_| "瀏覽器狀態鎖損壞".to_string())?;
        if state.tabs.is_empty() {
            return Err("瀏覽器沒有開著".to_string());
        }
        state.active
    };
    app.get_webview_window(&tab_label(id))
        .ok_or_else(|| "作用中分頁不存在".to_string())
}

/// 殼層導覽列按下去之後要做的事。`url` 只有 `goto` 會用到。
pub fn navigate<R: Runtime>(
    app: &AppHandle<R>,
    action: &str,
    url: Option<&str>,
) -> Result<(), String> {
    let outcome = navigate_inner(app, action, url);
    if let Err(e) = &outcome {
        eprintln!("[browser] 導覽 {action:?}（url={url:?}）失敗：{e}");
    }
    outcome
}

fn navigate_inner<R: Runtime>(
    app: &AppHandle<R>,
    action: &str,
    url: Option<&str>,
) -> Result<(), String> {
    let tab = active_tab(app)?;
    match action {
        "back" => go_history(&tab, true),
        "forward" => go_history(&tab, false),
        "reload" => tab.reload().map_err(|e| e.to_string()),
        "goto" => {
            let target = normalize_url(url.unwrap_or(""))?;
            tab.navigate(target).map_err(|e| e.to_string())
        }
        other => Err(format!("不認識的導覽動作：{other}")),
    }
}

// ─── 尺寸與位置記憶 ───────────────────────────────────────────────────────────

/// 記住的視窗幾何，實體像素。全帳號共用一組——這是「那個瀏覽器視窗」的大小，
/// 不是某個帳號的偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Geometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

const GEOMETRY_FILE: &str = "browser-window.json";

fn geometry_path<R: Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(GEOMETRY_FILE))
}

fn load_geometry<R: Runtime>(app: &AppHandle<R>) -> Option<Geometry> {
    let raw = std::fs::read_to_string(geometry_path(app)?).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_geometry<R: Runtime>(app: &AppHandle<R>, geometry: &Geometry) {
    let Some(path) = geometry_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(geometry) {
        let _ = std::fs::write(path, json);
    }
}

/// 一台螢幕的實體範圍：`(x, y, width, height)`。
type MonitorRect = (i32, i32, u32, u32);

/// 記住的位置還抓得到嗎——標題列中點要落在某台螢幕上。螢幕接拔或解析度改變後，
/// 舊座標可能整片落在畫面外，那時就得退回預設位置，否則視窗開出來抓不到也看不見。
fn titlebar_is_reachable(geometry: &Geometry, monitors: &[MonitorRect]) -> bool {
    let probe_x = geometry.x + (geometry.width / 2) as i32;
    let probe_y = geometry.y + (TOOLBAR_H / 2.0) as i32;
    monitors.iter().any(|&(mx, my, mw, mh)| {
        probe_x >= mx
            && probe_y >= my
            && probe_x < mx + mw as i32
            && probe_y < my + mh as i32
    })
}

fn monitor_rects<R: Runtime>(app: &AppHandle<R>) -> Vec<MonitorRect> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width, s.height)
        })
        .collect()
}

// ─── 版面 ─────────────────────────────────────────────────────────────────────

/// 分頁視窗在螢幕上的實體位置與大小：貼在工具列視窗畫出的框裡面（邊框內、工具列
/// 下方）。輸入是工具列視窗的實體位置、實體大小，以及 **CSS 像素換算實體像素的
/// 比率**（`css_to_px`，不是裸的 `scale_factor()`）。放不下就回 `None`
/// （最小化時 size 是 0×0）。
fn tab_rect_px(
    pos: (i32, i32),
    size: (u32, u32),
    px_per_css: f64,
) -> Option<(i32, i32, u32, u32)> {
    let px_per_css = if px_per_css > 0.0 { px_per_css } else { 1.0 };
    let edge = (EDGE * px_per_css).round() as i32;
    let top = (TOOLBAR_H * px_per_css).round() as i32;
    let w = size.0 as i32 - edge * 2;
    let h = size.1 as i32 - top - edge;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((pos.0 + edge, pos.1 + top, w as u32, h as u32))
}

/// 把所有分頁視窗排到工具列視窗的框裡。隱藏中的分頁也排——切過去時才不會先閃一下
/// 舊位置。
fn relayout_tabs<R: Runtime>(app: &AppHandle<R>) {
    let Some(toolbar) = toolbar_window(app) else { return };
    let (Ok(pos), Ok(size), Ok(scale)) = (
        toolbar.outer_position(),
        toolbar.inner_size(),
        toolbar.scale_factor(),
    ) else {
        return;
    };
    let Some((x, y, w, h)) =
        tab_rect_px((pos.x, pos.y), (size.width, size.height), css_to_px(scale))
    else {
        return;
    };
    let ids: Vec<u64> = {
        let Ok(state) = STATE.lock() else { return };
        state.tabs.iter().map(|t| t.id).collect()
    };
    for id in ids {
        if let Some(tab) = app.get_webview_window(&tab_label(id)) {
            let _ = tab.set_position(tauri::PhysicalPosition::new(x, y));
            let _ = tab.set_size(tauri::PhysicalSize::new(w, h));
        }
    }
}

// ─── 單一視窗組守門 ───────────────────────────────────────────────────────────

/// 目前視窗組歸屬的帳號 id。全 app 同時只有一組帳號瀏覽器（WebView2 同 app 共用
/// cookie 儲存區，兩個帳號同時開會互相踩掉登入態），所以這裡就是單一狀態。
static WINDOW_OWNER: Mutex<Option<String>> = Mutex::new(None);

/// 共用的 cookie 儲存區現在裝的是哪個帳號的東西。`None`＝不知道（程式剛啟動，
/// 資料夾裡留著的是上一次執行時最後開的那個帳號）。
///
/// 換了帳號就整個清空再注入：beanfun 的登入 cookie 我們認得、可以逐顆換掉，但
/// 活動頁那類子網域會發自己的 session cookie，名字、網域都不在我們手上——留著，
/// 下一個帳號開同一頁就可能沿用上一個人的 session。
static COOKIE_OWNER: Mutex<Option<String>> = Mutex::new(None);

/// 有一組視窗正在建、但還沒顯示出來。這段期間它答不出「我看得見」，沒有這個旗標
/// 就會被下一次開啟當成幽靈砍掉。
static OPENING: AtomicBool = AtomicBool::new(false);

/// 這個 `Destroyed` 是不是還在清自己那一組。拆出來可單測。
fn teardown_belongs_to(mine: u64, current: u64) -> bool {
    mine == current
}

/// 開啟流程的登記：判定當下就先佔住歸屬與「正在開」，中途 `?` 掉出去也要還原，
/// 否則簿記說某個帳號開著、桌面上卻什麼都沒有——那正是原本那個誤擋的形狀。
struct OpeningGuard {
    account: String,
    opened: bool,
}

impl Drop for OpeningGuard {
    fn drop(&mut self) {
        OPENING.store(false, Ordering::Release);
        if self.opened {
            return;
        }
        if let Ok(mut owner) = WINDOW_OWNER.lock() {
            // 只收自己那一筆：期間若已經換人登記，那是別人的視窗。
            if owner.as_deref() == Some(self.account.as_str()) {
                *owner = None;
            }
        }
        // 快照留著也沒有視窗會用，下一組開起來會整份覆寫；留著只是給後來讀
        // `state.cookies` 的人一份沒有主人的資料。
        if let Ok(mut state) = STATE.lock() {
            state.tabs.clear();
            state.active = 0;
            state.cookies.clear();
        }
    }
}

/// 一次開啟請求該怎麼處理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// 沒有視窗組（或上一組已經關掉）→ 開新的
    Open,
    /// 同一個帳號又點了一次 → 把既有視窗帶到前景
    Focus,
    /// 別的帳號的視窗還開著 → 拒絕，請使用者自己先關掉
    Refuse,
}

/// 那個視窗還真的在不在。**不能只看 `get_webview_window()` 有沒有回東西**：
/// WebView2 就是 Edge 的執行環境，Edge 在背景更新時會把執行中的 WebView2 連同
/// 視窗一起收掉，而 `Destroyed` 事件不保證跑得到我們的處理器——簿記裡就留下一個
/// 螢幕上根本不存在的幽靈，`WINDOW_OWNER` 也沒被清掉，之後開別的帳號一律被擋，
/// 而且要重開整個 app 才會好。所以改成去問視窗本人：只有活著的視窗答得出自己
/// 可不可見。最小化也算還在（`IsWindowVisible` 對最小化視窗仍為真，這裡兩個都
/// 收是為了不依賴那個細節）。
fn toolbar_is_alive<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    alive_from(window.is_visible().ok(), window.is_minimized().ok())
}

/// 拆出來可單測：問不到答案（`None`）＝視窗已經不在，不是「暫時不確定」。
fn alive_from(visible: Option<bool>, minimized: Option<bool>) -> bool {
    match (visible, minimized) {
        (Some(v), Some(m)) => v || m,
        _ => false,
    }
}

/// 守門判斷。`owner` 是目前視窗組歸屬的帳號、`alive` 是那組視窗還在不在。
/// 拒絕時**不代關**——使用者親自定的：跳警告請他自己關，別把他正在看的頁面關掉。
fn decide(owner: Option<&str>, requested: &str, alive: bool) -> Decision {
    match owner {
        Some(_) if !alive => Decision::Open,
        Some(o) if o == requested => Decision::Focus,
        Some(_) => Decision::Refuse,
        None => Decision::Open,
    }
}

// ─── 視窗 ─────────────────────────────────────────────────────────────────────

/// 開啟帶著這個帳號登入態的瀏覽器。同帳號已開著就帶到前景；別的帳號還開著
/// 就回 `BROWSER_STILL_OPEN`。
///
/// 必須從 `async` command 呼叫：建視窗要等主執行緒，同步情境會 deadlock。
pub fn open<R: Runtime>(
    app: &AppHandle<R>,
    account_id: &str,
    alias: &str,
    jar: &Arc<CookieStoreMutex>,
) -> Result<(), String> {
    let existing = toolbar_window(app);
    let alive = existing.as_ref().map(toolbar_is_alive).unwrap_or(false);
    // 判定與登記在同一個鎖裡完成，否則兩個帳號同時點會雙雙判到 Open。
    // `OPENING` 補的是視窗建好到 show 之間那段：那時它還看不見，會被下面的
    // 幽靈清理當成殘骸砍掉——而那正是使用者剛剛才點開的視窗。
    let decision = {
        let mut owner = WINDOW_OWNER.lock().map_err(|_| "瀏覽器狀態鎖損壞".to_string())?;
        let decision = decide(owner.as_deref(), account_id, alive || OPENING.load(Ordering::Acquire));
        if decision == Decision::Open {
            OPENING.store(true, Ordering::Release);
            *owner = Some(account_id.to_string());
        }
        decision
    };
    match decision {
        Decision::Focus => {
            if let Some(window) = existing {
                window.unminimize().ok();
                window.set_focus().map_err(|e| e.to_string())?;
                return Ok(());
            }
            // 判到 Focus 卻查不到視窗，只有兩種可能。前者無事：視窗還在建、
            // 還沒進簿記，這就是連點的第二下。
            if OPENING.load(Ordering::Acquire) {
                return Ok(());
            }
            // 後者是 `STATE` 鎖壞掉，`toolbar_window()` 因此回不出視窗。契約要求
            // 失敗要講出來——靜默 `Ok` 會讓使用者點了完全沒反應，還以為程式當了。
            eprintln!("[browser] 判定要帶到前景，卻查不到工具列視窗（狀態鎖可能已損壞）");
            return Err("瀏覽器狀態異常，請重新啟動程式".to_string());
        }
        Decision::Refuse => return Err("BROWSER_STILL_OPEN".to_string()),
        Decision::Open => {}
    }

    // 判定已經登記完，之後不管成功或中途失敗都要把登記還原。
    let mut opening = OpeningGuard { account: account_id.to_string(), opened: false };

    // 換代要在砍幽靈**之前**：舊視窗的 `Destroyed` 只要還認得出自己那一代，就會
    // 把 STATE 和歸屬清掉，而那時登記的已經是這一組了。先換代，晚到的事件就會
    // 看到號碼不對而自己收手。
    let generation = {
        let mut state = STATE.lock().map_err(|_| "瀏覽器狀態鎖損壞".to_string())?;
        state.generation += 1;
        state.generation
    };

    // 放行了就把上幾組的殘骸都收乾淨——舊工具列連同它的分頁一起掃，label 前綴就是
    // 認人的依據。分頁一定要掃：工具列被 Edge 收掉時它可能已經不在簿記裡，分頁卻
    // 還在，那些視窗沒有標題列、不進工作列，留下來就是一片關不掉的東西賴在桌面上。
    // 這裡**不等**它們真的消失（也等不到——見 `TOOLBAR_LABEL_PREFIX`），新視窗的
    // label 號碼本來就跟它們不同。
    let orphans: Vec<_> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| is_browser_label(label))
        .map(|(_, win)| win)
        .collect();
    for win in orphans {
        let _ = win.destroy();
    }

    // 沒有一顆 beanfun cookie＝這個 session 已經不帶登入態，開下去只會停在未登入
    // 首頁。先擋掉，讓使用者知道要重新登入，而不是自己去猜為什麼沒登入。
    let cookies = cookies_from_jar(jar);
    if !cookies.iter().any(|c| is_beanfun_domain(&c.domain)) {
        return Err("SESSION_EXPIRED".to_string());
    }
    // 只印網域、名字與旗標，不印值——那些是 session 憑證。彈窗沒登入時就靠這行看出
    // 該網域的 cookie 到底有沒有被帶進來。旗標要印，因為走 SSO 的 `checkin_step2`
    // 是用 `document.cookie` 讀 `bfWebToken` 的：只要它是 host-only 或 HttpOnly，
    // `tw.newlogin.beanfun.com` 就讀不到，整條 SSO 會被打回登入頁。
    let inventory: Vec<String> = cookies
        .iter()
        .map(|c| {
            let mut flags = String::new();
            if c.http_only { flags.push_str("|HttpOnly"); }
            if c.secure { flags.push_str("|Secure"); }
            format!("{}{}{}", c.domain, c.name, flags)
        })
        .collect();
    eprintln!("[browser] 準備注入 {} 顆 cookie：{}", cookies.len(), inventory.join(", "));

    {
        let mut state = STATE.lock().map_err(|_| "瀏覽器狀態鎖損壞".to_string())?;
        state.tabs.clear();
        state.active = 0;
        state.cookies = cookies;
        // ★這裡只「判斷」，不登記：儲存區要等真的清空了才算換了主人。先登記的話，
        // 這次開到一半失敗（清空沒發生），下一次再開同一個帳號就會被當成「沒換
        // 帳號」，上一個人留下的東西就這樣留著了。
        let owner = COOKIE_OWNER.lock().map_err(|_| "瀏覽器狀態鎖損壞".to_string())?;
        state.tidy = if owner.as_deref() == Some(account_id) {
            Tidy::SameNames
        } else {
            Tidy::Everything { account: account_id.to_string() }
        };
    }

    // 工具列視窗先隱藏著開，套用記住的幾何、排好版面才顯示——否則會先閃一下預設
    // 大小。transparent＝標題列上緣要圓角（圓弧外是桌面）；打字卡死與透明無關
    // （2026-08-18 已坐實是 multi-webview 的問題），這裡可以放心用。
    let shell_url = format!(
        "browser.html?alias={}&titlebar={TITLEBAR_H}&navbar={NAVBAR_H}&edge={EDGE}",
        urlencode(alias)
    );
    let label = toolbar_label(generation);
    let toolbar = WebviewWindowBuilder::new(app, label, WebviewUrl::App(shell_url.into()))
        .title(alias)
        .inner_size(DEFAULT_W, DEFAULT_H)
        .min_inner_size(MIN_W, MIN_H)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(true)
        .center()
        .visible(false)
        .build()
        .map_err(|e| format!("開啟瀏覽器視窗失敗：{e}"))?;

    // 系統文字大小放大的是頁面，視窗要跟著長大，否則版面被裁——跟主視窗同一條
    // 路徑（`win::size_for_text_scale`），連「不可大過工作區」的保護也共用：
    // 工具列的標題列是自己畫的，視窗一超出螢幕就抓不到、關不掉。
    // 開窗時是隱藏的，所以這裡改尺寸不會閃。
    #[cfg(windows)]
    if text_scale() > 1.0 {
        let work = crate::win::work_area_size();
        if let Ok(sz) = toolbar.inner_size() {
            let (w, h) =
                crate::win::size_for_text_scale((sz.width, sz.height), text_scale(), work);
            let _ = toolbar.set_size(tauri::PhysicalSize::new(w, h));
            let _ = toolbar.center();
        }
        if let Ok(dpi) = toolbar.scale_factor() {
            let min = ((MIN_W * dpi).round() as u32, (MIN_H * dpi).round() as u32);
            let (mw, mh) = crate::win::size_for_text_scale(min, text_scale(), work);
            let _ = toolbar.set_min_size(Some(tauri::PhysicalSize::new(mw, mh)));
        }
    }

    // 記住的幾何是實體像素、已經帶著使用者當時的文字倍率，所以直接套、不再乘一次。
    if let Some(saved) = load_geometry(app) {
        if titlebar_is_reachable(&saved, &monitor_rects(app)) {
            let _ = toolbar.set_size(tauri::PhysicalSize::new(saved.width, saved.height));
            let _ = toolbar.set_position(tauri::PhysicalPosition::new(saved.x, saved.y));
        }
    }

    // 工具列視窗一動（拖動、縮放、換螢幕）就把所有分頁視窗重新貼回框裡。分頁是
    // 獨立的 owned window，不會自己跟著 owner 跑。
    let app_for_events = app.clone();
    let toolbar_for_events = toolbar.clone();
    toolbar.on_window_event(move |event| match event {
        tauri::WindowEvent::Resized(_)
        | tauri::WindowEvent::Moved(_)
        | tauri::WindowEvent::ScaleFactorChanged { .. } => {
            relayout_tabs(&app_for_events);
        }
        // 趁視窗還活著把大小位置記下來（Destroyed 時已經問不到了）
        tauri::WindowEvent::CloseRequested { .. } => {
            if let (Ok(pos), Ok(size)) = (
                toolbar_for_events.outer_position(),
                toolbar_for_events.inner_size(),
            ) {
                save_geometry(
                    &app_for_events,
                    &Geometry { x: pos.x, y: pos.y, width: size.width, height: size.height },
                );
            }
        }
        // 工具列視窗沒了就把整組收掉：分頁視窗是 owned window，系統會跟著銷毀，
        // 這裡再補一次 destroy 是保險（也把 tauri 的簿記清乾淨），並清空狀態。
        tauri::WindowEvent::Destroyed => {
            // 這個事件可能晚到——晚到下一組視窗都開好了。那時 STATE 已經是別人的，
            // 清下去就是把新視窗的 cookie 和歸屬洗掉。
            // 鎖壞掉時就當作是自己的：收尾寧可多做一次，也不要靜靜不做——
            // 不做的話 `WINDOW_OWNER` 永遠留著，等於回到那個要重開程式才好的誤擋。
            let current = STATE.lock().map(|s| s.generation).unwrap_or(generation);
            if !teardown_belongs_to(generation, current) {
                return;
            }
            let ids: Vec<u64> = STATE
                .lock()
                .map(|s| s.tabs.iter().map(|t| t.id).collect())
                .unwrap_or_default();
            for id in ids {
                if let Some(tab) = app_for_events.get_webview_window(&tab_label(id)) {
                    let _ = tab.destroy();
                }
            }
            if let Ok(mut state) = STATE.lock() {
                state.tabs.clear();
                state.active = 0;
                state.cookies.clear();
            }
            if let Ok(mut owner) = WINDOW_OWNER.lock() {
                *owner = None;
            }
        }
        _ => {}
    });

    // 焦點在工具列（網址列）時 Ctrl+W 也要能關作用中的分頁
    hook_tab_shortcuts(&toolbar, app, None);
    toolbar.show().map_err(|e| e.to_string())?;

    // 第一個分頁：注入 cookie 完才導向首頁
    let home: Url = HOME_URL
        .parse()
        .map_err(|e| format!("起始頁網址無效：{e}"))?;
    open_tab(app, Some(home), None)?;
    opening.opened = true;
    Ok(())
}

/// 開一個新分頁。`navigate_to` 有值＝手動開的分頁（先注入 cookie 再導向）；
/// `None`＝網頁要求的新視窗（內容由 WebView2 的 `SetNewWindow` 灌進來，不導向）。
/// 回傳建好的分頁視窗（`on_new_window` 的 `Create` 要用）。
fn open_tab<R: Runtime>(
    app: &AppHandle<R>,
    navigate_to: Option<Url>,
    features: Option<NewWindowFeatures>,
) -> Result<WebviewWindow<R>, String> {
    let toolbar = toolbar_window(app).ok_or_else(|| "瀏覽器沒有開著".to_string())?;

    let id = {
        let mut state = STATE.lock().map_err(|_| "瀏覽器狀態鎖損壞".to_string())?;
        let id = state.next_id;
        state.next_id += 1;
        state.tabs.push(Tab { id, title: "載入中…".to_string(), url: String::new() });
        id
    };

    let app_for_title = app.clone();
    let app_for_load = app.clone();
    let app_for_nav = app.clone();
    let app_for_new_window = app.clone();
    let mut builder = WebviewWindowBuilder::new(app, tab_label(id), WebviewUrl::External(blank_url()))
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .visible(false)
        .initialization_script(THIN_SCROLLBAR_SCRIPT)
        // 分頁標題＝網頁標題。事件可能在導航前後亂序來，一律以最新為準。
        .on_document_title_changed(move |_window, title| {
            set_tab_title(&app_for_title, id, &title);
        })
        // 導航一開始就更新網址列——上一頁/下一頁這類歷史導航不會觸發
        // on_page_load，只靠它網址列會停在舊網址。一律放行（true）。
        .on_navigation(move |url| {
            tab_loaded(&app_for_nav, id, url.as_str());
            true
        })
        // 每次頁面載入完再記一次（重新導向後的最終網址以這份為準）
        .on_page_load(move |_window, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                tab_loaded(&app_for_load, id, payload.url().as_str());
            }
        })
        // 網頁要求開新視窗：帶尺寸的（金流小視窗）交給 WebView2 原生開、保
        // window.opener；一般連結/target=_blank 開成我們的分頁。`Create` 走
        // `SetNewWindow`，opener 一樣保留。wry 會把這個 handler 排進訊息迴圈
        // （deferral）再跑，所以在裡面建視窗不會 deadlock。
        .on_new_window(move |url, features| {
            if features.size().is_some() {
                return NewWindowResponse::Allow;
            }
            match open_tab(&app_for_new_window, None, Some(features)) {
                Ok(window) => NewWindowResponse::Create { window },
                Err(e) => {
                    // 分頁開不成寧可退回原生視窗，也不要把連結吞掉
                    eprintln!("[browser] 連結開分頁失敗，退回原生視窗：{e}（{url}）");
                    NewWindowResponse::Allow
                }
            }
        });

    // 網頁要求的新視窗必須沿用來源 webview 的 WebView2 environment（`Create` 的
    // 硬性要求），`window_features` 會把它接好。
    if let Some(features) = features {
        builder = builder.window_features(features);
    }
    #[cfg(windows)]
    {
        // owned window：恆在工具列視窗之上（分頁就是要疊在它的框裡）、跟著它
        // 最小化與銷毀、不出現在工作列。
        builder = builder
            .owner(&toolbar)
            .map_err(|e| format!("綁定瀏覽器視窗失敗：{e}"))?;
    }

    let tab = builder
        .build()
        .map_err(|e| format!("開啟分頁失敗：{e}"))?;

    hook_tab_shortcuts(&tab, app, Some(id));
    // 先排進框裡再顯示，不然會在預設位置閃一下
    relayout_tabs(app);

    if let Some(target) = navigate_to {
        // 手動開的分頁：cookie 注入完成才導向，網頁才不會在沒有登入態時先載入
        // 一次。注入是 profile 層級的，理論上注一次就夠，但注入便宜、到期時間又
        // 短（6h），每次開分頁都重注一次省得踩到過期。
        let (cookies, tidy) = {
            let mut state = STATE.lock().map_err(|_| "瀏覽器狀態鎖損壞".to_string())?;
            (state.cookies.clone(), std::mem::replace(&mut state.tidy, Tidy::Keep))
        };
        let expected = cookies.len();
        let nav_target = tab.clone();
        // ★注入之前，先把儲存區裡**同名卻掛在別處**的舊 cookie 拿掉。
        // 這個資料夾是所有帳號、所有 session 共用的，而注入只蓋得掉「名稱＋網域＋
        // 路徑」都一樣的那一顆：上一個 session 若把 `bfUID` 留在 `tw.beanfun.com`，
        // 這一次注進 `.beanfun.com` 的那顆蓋不到它，beanfun 就同時收到新舊兩個
        // `bfUID`，判成未登入（2026-09-19 實機：GamaPass 登入留下的錯位 cookie
        // 讓之後的 QR 登入開出來全是未登入）。要進來的那批才是現在登入的人。
        // 位置完全相同的那顆**不刪**，注入會直接蓋掉它——見 `CookieSlots`。
        let names: std::collections::HashSet<String> = cookies.iter().map(|c| c.name.clone()).collect();
        let slots = CookieSlots::of(&cookies);
        tab.with_webview(move |platform| {
            let seed = move |platform: &tauri::webview::PlatformWebview| {
                // 注入在主執行緒非同步跑，錯誤沒辦法回傳給呼叫端；印出來至少實機
                // 驗收時查得到——「網頁停在未登入首頁」的成因就在這行的結果裡。
                match inject_cookies(platform, &cookies) {
                    Ok(n) if n == expected => {}
                    Ok(n) => eprintln!("[browser] cookie 注入只成功 {n}/{expected} 顆"),
                    Err(e) => eprintln!("[browser] cookie 注入失敗（0/{expected} 顆）：{e}"),
                }
                when_cookies_settle(platform, move || {
                    let _ = nav_target.navigate(target);
                });
            };
            // 換了帳號：整個清空（見 `COOKIE_OWNER`）。同一個帳號：只換掉同名的，
            // 他在這個瀏覽器裡登過的其他網站不受影響。
            match tidy {
                Tidy::Everything { account } => {
                    wipe_cookies_then(platform, move |platform, clean| {
                        // 沒清乾淨＝裡面混著兩個人的東西，不是任何人的；下次不管開誰都要再清。
                        if let Ok(mut owner) = COOKIE_OWNER.lock() {
                            *owner = clean.then_some(account);
                        }
                        seed(platform);
                    });
                }
                Tidy::SameNames => {
                    clear_cookies_then(
                        platform,
                        SESSION_COOKIE_URLS,
                        move |name, domain, path| names.contains(name) && !slots.holds(name, domain, path),
                        move |platform, _| seed(platform),
                    );
                }
                Tidy::Keep => seed(&platform),
            }
        })
        .map_err(|e| format!("注入登入資訊失敗：{e}"))?;
    }

    activate_tab(app, id)?;
    Ok(tab)
}

/// 把登入 session 的 cookie 寫進 `window` 這顆 webview，然後才導向 `target`。
///
/// 給 `gamapass` 用（歸屬仍在本模組，見總表）：那個流程的登入態是 beanfun 綁在
/// 這條 session 上的，webview 沒帶著同一批 cookie 過去，OAuth 繞一圈回來時
/// beanfun 認不得自己發的 nonce，回「參數(nonce)驗證失敗」。
///
/// **先清掉 `stale` 這幾個網址底下的舊 cookie 再注入**：那個 webview 的資料夾是
/// 重複使用的，上一次登入留下的 token 還在裡面，portal 會拿舊的那條短路掉這一次
/// 的流程。★只清這幾個網址的，不是整個清空——對方網域上「保持登入狀態」留下的
/// 登入態也住在同一個資料夾裡，那正是下次不必再驗證的原因。
///
/// 注入要用的位置不刪（見 `CookieSlots`），注入會直接蓋掉。
pub(crate) fn seed_and_navigate<R: Runtime>(
    window: &WebviewWindow<R>,
    jar: &Arc<CookieStoreMutex>,
    stale: &'static [&'static str],
    target: Url,
) -> Result<(), String> {
    let cookies = cookies_from_jar(jar);
    let expected = cookies.len();
    let slots = CookieSlots::of(&cookies);
    let nav_target = window.clone();
    window
        .with_webview(move |platform| {
            let seed = move |platform: &tauri::webview::PlatformWebview| {
                // 注入在主執行緒非同步跑，錯誤回不到呼叫端；印出來至少查得到——
                // 「登入繞回來說 nonce 不對」的成因就在這幾行的結果裡。
                match inject_cookies(platform, &cookies) {
                    Ok(n) if n == expected => {}
                    Ok(n) => eprintln!("[browser] cookie 注入只成功 {n}/{expected} 顆"),
                    Err(e) => eprintln!("[browser] cookie 注入失敗（0/{expected} 顆）：{e}"),
                }
                when_cookies_settle(platform, move || {
                    let _ = nav_target.navigate(target);
                });
            };
            clear_cookies_then(
                platform,
                stale,
                move |name, domain, path| !slots.holds(name, domain, path),
                move |platform, _| seed(platform),
            );
        })
        .map_err(|e| format!("注入登入資訊失敗：{e}"))
}

/// 注入那批 cookie 會落在哪些位置（名稱＋網域＋路徑）。清舊 cookie 時這些位置
/// **一律不刪**。
///
/// ★`DeleteCookie` 送出去之後不保證比後面的 `AddOrUpdateCookie` 早生效。2026-09-20
/// 實機紀錄：同一個帳號連開四次，一、三次有登入，二、四次沒有——沒登入的那兩次，
/// 儲存區裡 `bfWebToken`／`bfUID`／`bfSecretCode` 整顆不見：上一次成功注入的那顆
/// 這次被排了刪除，刪除卻落在新注入之後，把剛寫進去的帶走了（而上一次失敗時
/// 儲存區沒有東西可刪，所以下一次又好了，於是一好一壞輪流）。位置相同的那顆根本
/// 不必刪，注入會蓋掉它；不刪，晚到的刪除就碰不到注入的東西。
#[derive(Clone)]
struct CookieSlots(std::collections::HashSet<(String, String, String)>);

impl CookieSlots {
    fn of(cookies: &[WebviewCookie]) -> Self {
        Self(cookies.iter().map(|c| Self::slot(&c.name, &c.domain, &c.path)).collect())
    }

    fn slot(name: &str, domain: &str, path: &str) -> (String, String, String) {
        (name.to_string(), domain.to_ascii_lowercase(), path.to_string())
    }

    fn holds(&self, name: &str, domain: &str, path: &str) -> bool {
        self.0.contains(&Self::slot(name, domain, path))
    }
}

/// 這顆 webview 的 cookie manager。
#[cfg(windows)]
fn cookie_manager(
    platform: &tauri::webview::PlatformWebview,
) -> Result<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2CookieManager, String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;
    use windows_core::Interface;

    unsafe {
        platform
            .controller()
            .CoreWebView2()
            .and_then(|core| core.cast::<ICoreWebView2_2>())
            .and_then(|core2| core2.CookieManager())
            .map_err(|e| format!("取不到 cookie manager：{e}"))
    }
}

/// 刪除送出去之後最多再讀幾輪來確認它真的生效了。
const CLEAR_VERIFY_ROUNDS: u32 = 30;

/// 逐顆讀出來、`doomed(名稱, 網域, 路徑)` 說要刪的才刪，**確認它們真的不見了**才呼叫
/// `then`。不用 `DeleteCookiesWithDomainAndPath`：那支的第一個參數是 cookie 的 name，
/// 「這個網域下全部」不是它的語意。
///
/// ★`DeleteCookie` 沒有完成通知，也不保證比後面的寫入早生效（見 `CookieSlots`），
/// 所以送出刪除之後要再讀，讀到那些 cookie 都不在了才往下走。同一顆 cookie **只送
/// 一次刪除**：`.beanfun.com` 的 cookie 在三個網址底下都讀得到，各送一次的話，第一個
/// 生效後看起來已經清乾淨，後面兩個卻還在路上，落在注入或網頁自己寫入之後。
///
/// `then` 的第二個參數＝要刪的確實都不在了。讀不到、或讀了 `CLEAR_VERIFY_ROUNDS`
/// 輪還在，都是 `false`，但照樣往下走：清不掉頂多是這一次被舊 token 短路，卡在這裡
/// 不導向則是整個登入開不起來。
#[cfg(windows)]
fn clear_cookies_then(
    platform: tauri::webview::PlatformWebview,
    urls: &'static [&'static str],
    doomed: impl Fn(&str, &str, &str) -> bool + Clone + 'static,
    then: impl FnOnce(&tauri::webview::PlatformWebview, bool) + 'static,
) {
    let manager = match cookie_manager(&platform) {
        Ok(manager) if !urls.is_empty() => manager,
        Ok(_) => return then(&platform, false),
        Err(e) => {
            eprintln!("[browser] {e}，舊 cookie 沒清");
            return then(&platform, false);
        }
    };
    let sweep = std::rc::Rc::new(Sweep {
        manager,
        urls,
        doomed: Box::new(doomed),
        wipe: false,
        sent: Default::default(),
        wiped: Default::default(),
        then: std::cell::RefCell::new(Some(Box::new(move |clean| then(&platform, clean)))),
    });
    Sweep::round(sweep, 0);
}

/// 整個儲存區清空，**確認空了**才呼叫 `then`（第二個參數＝真的空了）。換帳號用。
///
/// 這裡用 `DeleteAllCookies` 而不是逐顆刪：網頁內嵌框架（首頁的 YouTube 播放器）
/// 留下的分割 cookie，`DeleteCookie` 刪不到（2026-09-20 實機：`.youtube.com` 那五顆
/// 讀了幾十輪都還在）。它一樣沒有完成通知、會晚到，所以兩條：
/// - 送出之後一直讀到儲存區空了才往下走；
/// - **本來就是空的就不送**——那種情況沒有東西可以拿來確認它落地了沒，晚到的清空
///   會把剛注入的那批帶走。
#[cfg(windows)]
fn wipe_cookies_then(
    platform: tauri::webview::PlatformWebview,
    then: impl FnOnce(&tauri::webview::PlatformWebview, bool) + 'static,
) {
    let manager = match cookie_manager(&platform) {
        Ok(manager) => manager,
        Err(e) => {
            eprintln!("[browser] {e}，儲存區沒清");
            return then(&platform, false);
        }
    };
    let sweep = std::rc::Rc::new(Sweep {
        manager,
        urls: EVERY_COOKIE,
        doomed: Box::new(|_, _, _| true),
        wipe: true,
        sent: Default::default(),
        wiped: Default::default(),
        then: std::cell::RefCell::new(Some(Box::new(move |clean| then(&platform, clean)))),
    });
    Sweep::round(sweep, 0);
}

#[cfg(not(windows))]
fn wipe_cookies_then(
    platform: tauri::webview::PlatformWebview,
    then: impl FnOnce(&tauri::webview::PlatformWebview, bool) + 'static,
) {
    then(&platform, true);
}

/// 一次清除的進度。回呼全部跑在主執行緒上，所以用 `Rc` 就夠了。
#[cfg(windows)]
struct Sweep {
    manager: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2CookieManager,
    urls: &'static [&'static str],
    doomed: Box<dyn Fn(&str, &str, &str) -> bool>,
    /// 整個清空：不逐顆刪，改送一次 `DeleteAllCookies`（見 `wipe_cookies_then`）。
    wipe: bool,
    /// 那一次 `DeleteAllCookies` 送過了沒。
    wiped: std::cell::Cell<bool>,
    /// 已經送過刪除的位置，不再送第二次。
    sent: std::cell::RefCell<std::collections::HashSet<(String, String, String)>>,
    then: std::cell::RefCell<Option<Box<dyn FnOnce(bool)>>>,
}

#[cfg(windows)]
impl Sweep {
    fn finish(&self, clean: bool) {
        if let Some(then) = self.then.borrow_mut().take() {
            then(clean);
        }
    }

    /// 每個網址讀一次：沒送過刪除的就送，順便數還剩幾顆該刪的。一顆都不剩才算完。
    fn round(sweep: std::rc::Rc<Sweep>, attempt: u32) {
        use std::cell::Cell;
        use std::rc::Rc;
        use webview2_com::GetCookiesCompletedHandler;
        use windows_core::{HSTRING, PCWSTR};

        let pending = Rc::new(Cell::new(sweep.urls.len()));
        let left = Rc::new(Cell::new(0usize));
        let stuck = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let unread = Rc::new(Cell::new(false));
        let settle = {
            let (sweep, pending, left, unread) = (sweep.clone(), pending.clone(), left.clone(), unread.clone());
            let stuck = stuck.clone();
            move || {
                pending.set(pending.get() - 1);
                if pending.get() > 0 {
                    return;
                }
                if unread.get() {
                    sweep.finish(false);
                } else if left.get() == 0 {
                    sweep.finish(true);
                } else if attempt + 1 >= CLEAR_VERIFY_ROUNDS {
                    eprintln!("[browser] 還有 {} 顆舊 cookie 刪不掉：{}", left.get(), stuck.borrow().join(", "));
                    sweep.finish(false);
                } else {
                    if sweep.wipe && !sweep.wiped.replace(true) {
                        if let Err(e) = unsafe { sweep.manager.DeleteAllCookies() } {
                            eprintln!("[browser] 儲存區清不掉：{e}");
                            return sweep.finish(false);
                        }
                    }
                    Sweep::round(sweep.clone(), attempt + 1);
                }
            }
        };

        for url in sweep.urls {
            let uri = HSTRING::from(*url);
            let (sweep_cb, left_cb, unread_cb, settle_cb) = (sweep.clone(), left.clone(), unread.clone(), settle.clone());
            let stuck_cb = stuck.clone();
            let handler = GetCookiesCompletedHandler::create(Box::new(move |_result, list| {
                match list {
                    None => unread_cb.set(true),
                    Some(list) => {
                        let mut count = 0u32;
                        let _ = unsafe { list.Count(&mut count) };
                        for i in 0..count {
                            let Ok(cookie) = (unsafe { list.GetValueAtIndex(i) }) else { continue };
                            let mut name = windows_core::PWSTR::null();
                            let mut domain = windows_core::PWSTR::null();
                            let mut path = windows_core::PWSTR::null();
                            if unsafe { cookie.Name(&mut name) }.is_err()
                                || unsafe { cookie.Domain(&mut domain) }.is_err()
                                || unsafe { cookie.Path(&mut path) }.is_err()
                            {
                                continue;
                            }
                            let (name, domain, path) = (
                                webview2_com::take_pwstr(name),
                                webview2_com::take_pwstr(domain),
                                webview2_com::take_pwstr(path),
                            );
                            if !(sweep_cb.doomed)(&name, &domain, &path) {
                                continue;
                            }
                            left_cb.set(left_cb.get() + 1);
                            stuck_cb.borrow_mut().push(format!("{name}@{domain}{path}"));
                            if !sweep_cb.wipe && sweep_cb.sent.borrow_mut().insert(CookieSlots::slot(&name, &domain, &path)) {
                                let _ = unsafe { sweep_cb.manager.DeleteCookie(&cookie) };
                            }
                        }
                    }
                }
                settle_cb();
                Ok(())
            }));
            if let Err(e) = unsafe { sweep.manager.GetCookies(PCWSTR(uri.as_ptr()), &handler) } {
                eprintln!("[browser] 讀 {url} 的 cookie 失敗，沒清成：{e}");
                unread.set(true);
                settle();
            }
        }
    }
}

/// 等前面排進去的 cookie 寫入都處理完才呼叫 `then`：讀一次 cookie，回呼回來時
/// 先前的寫入已經過了同一個 cookie manager。問不到就直接往下走。
#[cfg(windows)]
fn when_cookies_settle(platform: &tauri::webview::PlatformWebview, then: impl FnOnce() + 'static) {
    use std::cell::RefCell;
    use std::rc::Rc;
    use webview2_com::GetCookiesCompletedHandler;
    use windows_core::{HSTRING, PCWSTR};

    let then = Rc::new(RefCell::new(Some(then)));
    let fire = {
        let then = then.clone();
        move || {
            if let Some(then) = then.borrow_mut().take() {
                then();
            }
        }
    };
    let Ok(manager) = cookie_manager(platform) else {
        fire();
        return;
    };
    let uri = HSTRING::from(HOME_URL);
    let fire_cb = fire.clone();
    let handler = GetCookiesCompletedHandler::create(Box::new(move |_result, _list| {
        fire_cb();
        Ok(())
    }));
    if unsafe { manager.GetCookies(PCWSTR(uri.as_ptr()), &handler) }.is_err() {
        fire();
    }
}

#[cfg(not(windows))]
fn when_cookies_settle(_platform: &tauri::webview::PlatformWebview, then: impl FnOnce() + 'static) {
    then();
}

#[cfg(not(windows))]
fn clear_cookies_then(
    platform: tauri::webview::PlatformWebview,
    _urls: &'static [&'static str],
    _doomed: impl Fn(&str, &str, &str) -> bool + Clone + 'static,
    then: impl FnOnce(&tauri::webview::PlatformWebview, bool) + 'static,
) {
    then(&platform, true);
}

/// 切到某個分頁：顯示它、藏起其他的，並把它的網址推給網址列。
fn activate_tab<R: Runtime>(app: &AppHandle<R>, id: u64) -> Result<(), String> {
    let (ids, url) = {
        let mut state = STATE.lock().map_err(|_| "瀏覽器狀態鎖損壞".to_string())?;
        if !state.tabs.iter().any(|t| t.id == id) {
            return Err("分頁不存在".to_string());
        }
        state.active = id;
        let ids: Vec<u64> = state.tabs.iter().map(|t| t.id).collect();
        let url = state
            .tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.url.clone())
            .unwrap_or_default();
        (ids, url)
    };
    for other in ids {
        if let Some(win) = app.get_webview_window(&tab_label(other)) {
            if other == id {
                let _ = win.show();
                let _ = win.set_focus();
            } else {
                let _ = win.hide();
            }
        }
    }
    emit_nav(app, &url);
    emit_tabs(app);
    Ok(())
}

/// 關掉一個分頁。關掉最後一個分頁＝關掉整個瀏覽器（使用者拍板的預設）。
fn close_tab<R: Runtime>(app: &AppHandle<R>, id: u64) -> Result<(), String> {
    let (ids, active) = {
        let mut state = STATE.lock().map_err(|_| "瀏覽器狀態鎖損壞".to_string())?;
        // 已經關掉的 id（× 被連點兩下）直接無視——走下去會被誤判成「最後一個
        // 分頁沒了」而把整個瀏覽器關掉。
        if !state.tabs.iter().any(|t| t.id == id) {
            return Ok(());
        }
        let ids: Vec<u64> = state.tabs.iter().map(|t| t.id).collect();
        let active = state.active;
        state.tabs.retain(|t| t.id != id);
        (ids, active)
    };
    if let Some(win) = app.get_webview_window(&tab_label(id)) {
        let _ = win.destroy();
    }
    match next_active_after_close(&ids, id, active) {
        Some(next) => {
            if next != active {
                activate_tab(app, next)?;
            } else {
                emit_tabs(app);
            }
            Ok(())
        }
        None => {
            // 最後一個分頁沒了：走 close() 讓 CloseRequested 把幾何記下來
            if let Some(toolbar) = toolbar_window(app) {
                let _ = toolbar.close();
            }
            Ok(())
        }
    }
}

/// 殼層分頁列的動作入口。
pub fn tab_command<R: Runtime>(
    app: &AppHandle<R>,
    action: &str,
    id: Option<u64>,
) -> Result<(), String> {
    let outcome = match action {
        "new" => {
            let home: Url = HOME_URL
                .parse()
                .map_err(|e| format!("起始頁網址無效：{e}"))?;
            open_tab(app, Some(home), None).map(|_| ())
        }
        "activate" => activate_tab(app, id.ok_or("缺分頁 id")?),
        "close" => close_tab(app, id.ok_or("缺分頁 id")?),
        other => Err(format!("不認識的分頁動作：{other}")),
    };
    if let Err(e) = &outcome {
        eprintln!("[browser] 分頁 {action:?}（id={id:?}）失敗：{e}");
    }
    outcome
}

fn set_tab_title<R: Runtime>(app: &AppHandle<R>, id: u64, title: &str) {
    if let Ok(mut state) = STATE.lock() {
        if let Some(tab) = state.tabs.iter_mut().find(|t| t.id == id) {
            tab.title = if title.trim().is_empty() {
                "（無標題）".to_string()
            } else {
                title.to_string()
            };
        }
    }
    emit_tabs(app);
}

fn tab_loaded<R: Runtime>(app: &AppHandle<R>, id: u64, url: &str) {
    // about:blank 是分頁的胚胎狀態，不是使用者看得懂的網址，別推進網址列
    if url == "about:blank" {
        return;
    }
    let is_active = {
        let Ok(mut state) = STATE.lock() else { return };
        if let Some(tab) = state.tabs.iter_mut().find(|t| t.id == id) {
            tab.url = url.to_string();
        }
        state.active == id
    };
    if is_active {
        emit_nav(app, url);
    }
}

/// 在這顆 webview 攔分頁快捷鍵：Ctrl+W 關分頁（`tab` 給 `None`＝關作用中的）、
/// Ctrl+T 開新分頁。攔截機制與「為什麼在這層」見 [`crate::keyhook`]。
/// 動作丟到另一條執行緒跑——在 WebView2 回呼裡直接建/銷毀視窗是重入，會出事。
fn hook_tab_shortcuts<R: Runtime>(win: &WebviewWindow<R>, app: &AppHandle<R>, tab: Option<u64>) {
    let app = app.clone();
    crate::keyhook::hook_keys(win, move |key| {
        if !key.ctrl {
            return false;
        }
        let app = app.clone();
        match key.vk {
            w if w == u32::from(b'W') => {
                std::thread::spawn(move || {
                    let id = tab.or_else(|| STATE.lock().ok().map(|s| s.active));
                    if let Some(id) = id {
                        let _ = close_tab(&app, id);
                    }
                });
                true
            }
            t if t == u32::from(b'T') => {
                std::thread::spawn(move || {
                    let _ = tab_command(&app, "new", None);
                });
                true
            }
            _ => false,
        }
    });
}

fn blank_url() -> Url {
    "about:blank".parse().expect("about:blank is a valid URL")
}

/// 只 escape 會破壞 query string 的字元；別名是使用者自己打的中文。
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ─── Cookie 讀取（WebView2） ──────────────────────────────────────────────────

/// 從 webview 讀出來的一顆 cookie，連同它住在哪裡。
///
/// ★網域與路徑一定要帶著走：撈回來的 cookie 之後會再被注入別的 webview，掛錯
/// 網域的複本蓋不掉、也不會被正確的那一顆蓋掉，兩顆一起送出去就是未登入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeenCookie {
    pub name: String,
    pub value: String,
    /// WebView2 的寫法：跨子網域的帶前導點（`.beanfun.com`），host-only 的不帶。
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
}

impl SeenCookie {
    /// 這顆 cookie 當初的 `Set-Cookie` 長什麼樣，以及是哪個網址發的——交給 cookie
    /// jar 照這兩樣收下，它在 jar 裡的歸屬就跟在瀏覽器裡一模一樣。
    pub fn as_set_cookie(&self) -> (String, String) {
        let host = self.domain.trim_start_matches('.');
        let mut line = format!("{}={}; Path={}", self.name, self.value, self.path);
        if self.domain.starts_with('.') {
            line.push_str(&format!("; Domain={host}"));
        }
        if self.secure {
            line.push_str("; Secure");
        }
        if self.http_only {
            line.push_str("; HttpOnly");
        }
        (line, format!("https://{host}/"))
    }
}

/// 從某顆 webview 讀出 `url` 適用的 cookie。
///
/// 給 `gamapass` 用：那條登入的結果只會落在那顆 webview 的 cookie 儲存區裡，
/// 我們自己的 HTTP client 拿不到，只能過來撈。
///
/// WebView2 的讀取是非同步的、回呼跑在主執行緒，所以這裡用一個 channel 等它；
/// 等不到就當作沒有，呼叫端會再試下一輪。
#[cfg(windows)]
pub(crate) fn read_cookies<R: Runtime>(
    window: &WebviewWindow<R>,
    url: &str,
) -> Result<Vec<SeenCookie>, String> {
    use std::sync::mpsc;
    use webview2_com::GetCookiesCompletedHandler;
    use windows_core::{HSTRING, PCWSTR};

    let (tx, rx) = mpsc::sync_channel::<Vec<SeenCookie>>(1);
    let uri = HSTRING::from(url);
    window
        .with_webview(move |platform| unsafe {
            let manager = match cookie_manager(&platform) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[browser] {e}");
                    let _ = tx.send(Vec::new());
                    return;
                }
            };

            let tx = tx.clone();
            let handler = GetCookiesCompletedHandler::create(Box::new(move |_result, list| {
                let mut out = Vec::new();
                if let Some(list) = list {
                    let mut count = 0u32;
                    let _ = list.Count(&mut count);
                    for i in 0..count {
                        let Ok(cookie) = list.GetValueAtIndex(i) else { continue };
                        let mut name = windows_core::PWSTR::null();
                        let mut value = windows_core::PWSTR::null();
                        let mut domain = windows_core::PWSTR::null();
                        let mut path = windows_core::PWSTR::null();
                        if cookie.Name(&mut name).is_err()
                            || cookie.Value(&mut value).is_err()
                            || cookie.Domain(&mut domain).is_err()
                            || cookie.Path(&mut path).is_err()
                        {
                            continue;
                        }
                        let mut secure = windows_core::BOOL(0);
                        let mut http_only = windows_core::BOOL(0);
                        let _ = cookie.IsSecure(&mut secure);
                        let _ = cookie.IsHttpOnly(&mut http_only);
                        out.push(SeenCookie {
                            name: webview2_com::take_pwstr(name),
                            value: webview2_com::take_pwstr(value),
                            domain: webview2_com::take_pwstr(domain),
                            path: webview2_com::take_pwstr(path),
                            secure: secure.as_bool(),
                            http_only: http_only.as_bool(),
                        });
                    }
                }
                let _ = tx.send(out);
                Ok(())
            }));
            if let Err(e) = manager.GetCookies(PCWSTR(uri.as_ptr()), &handler) {
                eprintln!("[browser] 讀 cookie 失敗：{e}");
            }
        })
        .map_err(|e| format!("讀取 cookie 失敗：{e}"))?;

    Ok(rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap_or_default())
}

#[cfg(not(windows))]
pub(crate) fn read_cookies<R: Runtime>(
    _window: &WebviewWindow<R>,
    _url: &str,
) -> Result<Vec<SeenCookie>, String> {
    Ok(Vec::new())
}

/// 在 `window` 的頁面裡跑一段腳本，拿回它的結果（JSON 字串）。
///
/// 給 `gamapass` 用：那顆視窗零 IPC，頁面沒有辦法主動告訴我們什麼，所以由這邊
/// 去問。問不到（頁面正在換、逾時）一律回 `None`，呼叫端下一輪再問就是了。
#[cfg(windows)]
pub(crate) fn eval_json<R: Runtime>(window: &WebviewWindow<R>, script: &str) -> Option<String> {
    use std::sync::mpsc;
    use webview2_com::ExecuteScriptCompletedHandler;
    use windows_core::{HSTRING, PCWSTR};

    let (tx, rx) = mpsc::sync_channel::<Option<String>>(1);
    let script = HSTRING::from(script);
    window
        .with_webview(move |platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else {
                let _ = tx.send(None);
                return;
            };
            let reply = tx.clone();
            let handler = ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
                let _ = reply.send(result.ok().map(|_| json));
                Ok(())
            }));
            if core.ExecuteScript(PCWSTR(script.as_ptr()), &handler).is_err() {
                let _ = tx.send(None);
            }
        })
        .ok()?;
    rx.recv_timeout(std::time::Duration::from_secs(2)).ok().flatten()
}

#[cfg(not(windows))]
pub(crate) fn eval_json<R: Runtime>(_window: &WebviewWindow<R>, _script: &str) -> Option<String> {
    None
}

// ─── Cookie 注入（WebView2） ──────────────────────────────────────────────────

/// 把 cookie 寫進這顆 webview 的 WebView2 cookie 儲存區。回傳成功寫入的顆數；
/// 連 cookie manager 都拿不到才回 `Err`。
#[cfg(windows)]
fn inject_cookies(
    platform: &tauri::webview::PlatformWebview,
    cookies: &[WebviewCookie],
) -> Result<usize, String> {
    use windows_core::{HSTRING, PCWSTR};

    let manager = cookie_manager(platform)?;

    // 給一個明確的到期時間，讓 cookie 變成 persistent 而不是 session cookie：
    // session cookie 只活在建立它的那顆 webview 裡，其他分頁與網頁彈出的子視窗
    // 看不到。有效期給得短（幾小時），反正每次開分頁都重新注入。
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() + INJECTED_COOKIE_TTL_SECS)
        .ok();

    let mut written = 0usize;
    for c in cookies {
        let name = HSTRING::from(c.name.as_str());
        let value = HSTRING::from(c.value.as_str());
        let domain = HSTRING::from(c.domain.as_str());
        let path = HSTRING::from(c.path.as_str());
        unsafe {
            let created = manager.CreateCookie(
                PCWSTR(name.as_ptr()),
                PCWSTR(value.as_ptr()),
                PCWSTR(domain.as_ptr()),
                PCWSTR(path.as_ptr()),
            );
            let Ok(cookie) = created else { continue };
            let _ = cookie.SetIsSecure(c.secure);
            let _ = cookie.SetIsHttpOnly(c.http_only);
            if let Some(expires) = expires {
                let _ = cookie.SetExpires(expires);
            }
            if manager.AddOrUpdateCookie(&cookie).is_ok() {
                written += 1;
            }
        }
    }
    Ok(written)
}

#[cfg(not(windows))]
fn inject_cookies(
    _platform: &tauri::webview::PlatformWebview,
    _cookies: &[WebviewCookie],
) -> Result<usize, String> {
    Err("只支援 Windows".to_string())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_injected_slot_is_never_cleared_but_a_misplaced_twin_is() {
        let slots = CookieSlots::of(&[WebviewCookie {
            name: "bfUID".into(),
            value: "v".into(),
            domain: ".beanfun.com".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
        }]);
        assert!(slots.holds("bfUID", ".BEANFUN.com", "/"));
        assert!(!slots.holds("bfUID", "tw.beanfun.com", "/"));
        assert!(!slots.holds("bfUID", ".beanfun.com", "/sub"));
        assert!(!slots.holds("other", ".beanfun.com", "/"));
    }

    fn seen(name: &str, domain: &str, http_only: bool) -> SeenCookie {
        SeenCookie {
            name: name.into(),
            value: "v".into(),
            domain: domain.into(),
            path: "/".into(),
            secure: true,
            http_only,
        }
    }

    /// 撈回來的 cookie 要照它原本的歸屬還原：跨子網域的帶 Domain，host-only 的
    /// 不帶、由來源網址決定。全部掛到同一台主機上，就是 v2.4.0 那個 bug。
    #[test]
    fn a_seen_cookie_goes_back_where_it_lived() {
        assert_eq!(
            seen("bfUID", ".beanfun.com", true).as_set_cookie(),
            ("bfUID=v; Path=/; Domain=beanfun.com; Secure; HttpOnly".to_string(), "https://beanfun.com/".to_string())
        );
        assert_eq!(
            seen("GamaLoginSession", "login.beanfun.com", false).as_set_cookie(),
            ("GamaLoginSession=v; Path=/; Secure".to_string(), "https://login.beanfun.com/".to_string())
        );
    }

    fn jar_cookie(name: &str, domain: &str, kind: DomainKind) -> JarCookie {
        JarCookie {
            name: name.to_string(),
            value: format!("{name}-value"),
            domain: domain.to_string(),
            kind,
            path: "/".to_string(),
            secure: false,
            http_only: false,
        }
    }

    /// 照 `Set-Cookie` 的樣子塞進真的 cookie store，用來驗 jar 讀取那一段。
    fn store_with(set_cookies: &[&str], from: &str) -> cookie_store::CookieStore {
        let url = from.parse().expect("test URL");
        let mut store = cookie_store::CookieStore::default();
        for raw in set_cookies {
            store
                .parse(raw, &url)
                .unwrap_or_else(|e| panic!("test cookie {raw:?} rejected: {e}"));
        }
        store
    }

    #[test]
    fn beanfun_domains_are_injectable() {
        assert!(is_beanfun_domain("beanfun.com"));
        assert!(is_beanfun_domain("tw.beanfun.com"));
        assert!(is_beanfun_domain("login.beanfun.com"));
        assert!(is_beanfun_domain(".beanfun.com"));
        assert!(is_beanfun_domain("TW.BeanFun.com"));
    }

    #[test]
    fn other_domains_are_not_injectable() {
        assert!(!is_beanfun_domain("gamania.com"));
        assert!(!is_beanfun_domain("example.com"));
        // 前綴湊出來的相似網域不算 beanfun 的子網域
        assert!(!is_beanfun_domain("notbeanfun.com"));
        assert!(!is_beanfun_domain("beanfun.com.evil.net"));
    }

    #[test]
    fn a_cookie_without_a_domain_attribute_reads_back_as_host_only() {
        let store = store_with(&["ASP.NET_SessionId=abc; Path=/"], "https://tw.beanfun.com/");
        let read = read_jar(&store);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].domain, "tw.beanfun.com");
        assert_eq!(read[0].kind, DomainKind::Host);
        assert_eq!(read[0].value, "abc");
        assert_eq!(read[0].path, "/");
    }

    #[test]
    fn a_domain_attribute_reads_back_as_suffix() {
        let store = store_with(
            &["bfWebToken=tok; Domain=beanfun.com; Path=/"],
            "https://tw.beanfun.com/",
        );
        let read = read_jar(&store);
        assert_eq!(read[0].kind, DomainKind::Suffix);
        assert_eq!(read[0].domain, "beanfun.com");
    }

    #[test]
    fn secure_and_http_only_default_to_false_when_absent() {
        let store = store_with(&["plain=1; Path=/"], "https://tw.beanfun.com/");
        let read = read_jar(&store);
        assert!(!read[0].secure);
        assert!(!read[0].http_only);

        let store = store_with(
            &["guarded=1; Path=/; Secure; HttpOnly"],
            "https://tw.beanfun.com/",
        );
        let read = read_jar(&store);
        assert!(read[0].secure);
        assert!(read[0].http_only);
    }

    #[test]
    fn expired_cookies_never_reach_the_injection_list() {
        // jar 連過期的 cookie 都不收（parse 就退回），所以注入端不必自己濾時間。
        let url = "https://tw.beanfun.com/".parse().expect("test URL");
        let mut store = cookie_store::CookieStore::default();
        store.parse("alive=1; Path=/", &url).expect("live cookie");
        assert!(store
            .parse("dead=1; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT", &url)
            .is_err());

        let names: Vec<String> = read_jar(&store).into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["alive"]);
    }

    #[test]
    fn every_cookie_in_the_jar_gets_injected() {
        // 不按網域挑：只注 beanfun 的話，網頁點出去的彈窗（走 gamania 關聯網域）
        // 就沒有登入態——被丟掉的正是那些 cookie。
        let jar = vec![
            jar_cookie("bfWebToken", ".beanfun.com", DomainKind::Suffix),
            jar_cookie("sso", "gamania.com", DomainKind::Suffix),
            jar_cookie("ASP.NET_SessionId", "tw.beanfun.com", DomainKind::Host),
        ];
        let kept = select_for_injection(&jar);
        let names: Vec<&str> = kept.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["bfWebToken", "sso", "ASP.NET_SessionId"]);
    }

    #[test]
    fn bf_web_token_is_injected_without_http_only_so_the_sso_page_can_read_it() {
        // checkin_step2.aspx 的 DealWebToken() 用 document.cookie 讀 bfWebToken，
        // 讀不到就把使用者打回掃碼登入頁——照抄 HttpOnly 等於關掉活動頁登入。
        let http_only = |name: &str| JarCookie {
            http_only: true,
            ..jar_cookie(name, ".beanfun.com", DomainKind::Suffix)
        };
        let out = select_for_injection(&[http_only("bfWebToken"), http_only("bfUID")]);
        assert!(!out[0].http_only, "bfWebToken 必須讓網頁 JS 讀得到");
        // 其餘的照抄，忠實度只在有證據的那一顆讓步。
        assert!(out[1].http_only);
    }

    #[test]
    fn a_session_still_counts_as_logged_in_by_its_beanfun_cookies() {
        // 「還帶不帶登入態」仍然只看 beanfun 網域——只剩別家的 cookie 不算登入
        let only_others = vec![jar_cookie("sso", "gamania.com", DomainKind::Suffix)];
        let injected = select_for_injection(&only_others);
        assert!(!injected.iter().any(|c| is_beanfun_domain(&c.domain)));

        let with_beanfun = vec![jar_cookie("bfWebToken", ".beanfun.com", DomainKind::Suffix)];
        let injected = select_for_injection(&with_beanfun);
        assert!(injected.iter().any(|c| is_beanfun_domain(&c.domain)));
    }

    #[test]
    fn suffix_cookies_get_a_leading_dot_and_host_only_ones_do_not() {
        let jar = vec![
            jar_cookie("wide", "beanfun.com", DomainKind::Suffix),
            jar_cookie("narrow", "tw.beanfun.com", DomainKind::Host),
        ];
        let out = select_for_injection(&jar);
        assert_eq!(out[0].domain, ".beanfun.com");
        assert_eq!(out[1].domain, "tw.beanfun.com");
    }

    #[test]
    fn an_already_dotted_suffix_domain_does_not_gain_a_second_dot() {
        let jar = vec![jar_cookie("t", ".beanfun.com", DomainKind::Suffix)];
        assert_eq!(select_for_injection(&jar)[0].domain, ".beanfun.com");
    }

    #[test]
    fn carries_value_path_and_flags_through() {
        // 樣本刻意不用 bfWebToken——那顆是 HttpOnly 的例外，見 is_js_readable_cookie。
        let jar = vec![JarCookie {
            name: "bfSecretCode".into(),
            value: "abc123".into(),
            domain: ".beanfun.com".into(),
            kind: DomainKind::Suffix,
            path: "/beanfun_block".into(),
            secure: true,
            http_only: true,
        }];
        let out = select_for_injection(&jar);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].value, "abc123");
        assert_eq!(out[0].path, "/beanfun_block");
        assert!(out[0].secure);
        assert!(out[0].http_only);
    }

    #[test]
    fn tabs_sit_below_the_toolbar_and_inside_the_frame() {
        // 100% 縮放：內縮就是常數本身
        let r = tab_rect_px((100, 50), (1230, 720), 1.0).unwrap();
        assert_eq!(r, (100 + 3, 50 + 84, 1230 - 6, 720 - 84 - 3));
    }

    #[test]
    fn tab_rect_scales_the_insets_with_dpi() {
        // 150% 縮放：邊框與工具列高度都要放大，否則分頁會壓到框
        let r = tab_rect_px((0, 0), (1845, 1080), 1.5).unwrap();
        let edge = (3.0f64 * 1.5).round() as i32;
        let top = (84.0f64 * 1.5).round() as i32;
        assert_eq!(r, (edge, top, (1845 - edge as u32 * 2), (1080 - top as u32 - edge as u32)));
    }

    #[test]
    fn a_window_too_small_for_a_tab_has_no_rect() {
        // 最小化時回報 0×0；沒有可放分頁的空間就不排
        assert!(tab_rect_px((0, 0), (0, 0), 1.0).is_none());
        assert!(tab_rect_px((0, 0), (1230, 87), 1.0).is_none());
        assert!(tab_rect_px((0, 0), (6, 720), 1.0).is_none());
    }

    #[test]
    fn a_nonsense_scale_factor_falls_back_to_one() {
        let r = tab_rect_px((0, 0), (1230, 720), 0.0).unwrap();
        assert_eq!(r, (3, 84, 1224, 633));
    }

    #[test]
    fn closing_a_background_tab_keeps_the_active_one() {
        assert_eq!(next_active_after_close(&[1, 2, 3], 3, 1), Some(1));
    }

    #[test]
    fn closing_the_active_tab_moves_to_its_right_neighbor() {
        assert_eq!(next_active_after_close(&[1, 2, 3], 2, 2), Some(3));
    }

    #[test]
    fn closing_the_last_positioned_active_tab_moves_left() {
        assert_eq!(next_active_after_close(&[1, 2, 3], 3, 3), Some(2));
    }

    #[test]
    fn closing_the_only_tab_closes_the_browser() {
        assert_eq!(next_active_after_close(&[7], 7, 7), None);
    }

    #[test]
    fn a_stale_close_keeps_the_current_active_tab() {
        // × 被連點兩下：第二下的 id 已不在清單裡（close_tab 另有先行檢查直接
        // 回 Ok）。helper 對這種輸入不得回 None——None 的語意只保留給「真的
        // 關掉最後一個分頁」，誤回 None 會把整個瀏覽器關掉。
        assert_eq!(next_active_after_close(&[2, 3], 1, 2), Some(2));
    }

    const FHD: MonitorRect = (0, 0, 1920, 1080);

    #[test]
    fn a_full_url_is_taken_as_is() {
        assert_eq!(
            normalize_url("https://tw.beanfun.com/x").unwrap().as_str(),
            "https://tw.beanfun.com/x"
        );
        assert_eq!(
            normalize_url("  http://example.com  ").unwrap().as_str(),
            "http://example.com/"
        );
    }

    #[test]
    fn a_bare_host_gets_https() {
        assert_eq!(
            normalize_url("tw.beanfun.com").unwrap().as_str(),
            "https://tw.beanfun.com/"
        );
    }

    /// 這個結果是「拿去 Google 搜這串字」嗎——網址列的安全語意靠它驗。
    fn is_google_search_for(url: &Url, query: &str) -> bool {
        url.scheme() == "https"
            && url.host_str() == Some("www.google.com")
            && url.path() == "/search"
            && url
                .query_pairs()
                .any(|(k, v)| k == "q" && v == query)
    }

    #[test]
    fn non_http_schemes_become_searches_not_navigations() {
        // 網址列仍然不是任意 scheme 的入口：這類輸入變成搜尋字串、不被執行
        for raw in ["file:///C:/Windows/win.ini", "javascript:alert(1)", "gamaniagames://whatever"] {
            let url = normalize_url(raw).unwrap();
            assert!(is_google_search_for(&url, raw), "{raw} 應該變成搜尋，卻是 {url}");
        }
    }

    #[test]
    fn things_that_are_not_urls_become_google_searches() {
        let url = normalize_url("楓之谷 怎麼玩").unwrap();
        assert!(is_google_search_for(&url, "楓之谷 怎麼玩"));
        // 百分比編碼交給 query_pairs，中文不會壞
        assert!(url.as_str().contains("q=%E6%A5%93%E4%B9%8B%E8%B0%B7"));

        let url = normalize_url("beanfun").unwrap();
        assert!(is_google_search_for(&url, "beanfun"));

        // 有 http 前綴但 parse 不過（主機帶空白）＝也不是網址
        let url = normalize_url("https://a b").unwrap();
        assert!(is_google_search_for(&url, "https://a b"));
    }

    #[test]
    fn empty_input_is_still_an_error_not_a_search() {
        assert!(normalize_url("").is_err());
        assert!(normalize_url("   ").is_err());
    }

    #[test]
    fn a_window_on_screen_keeps_its_remembered_place() {
        let g = Geometry { x: 300, y: 200, width: 1100, height: 720 };
        assert!(titlebar_is_reachable(&g, &[FHD]));
    }

    #[test]
    fn a_window_left_off_screen_by_a_resolution_change_is_rejected() {
        // 上一次存的位置在第二台螢幕上，那台已經拔掉了
        let g = Geometry { x: 2400, y: 300, width: 1100, height: 720 };
        assert!(!titlebar_is_reachable(&g, &[FHD]));
        // 整片落在畫面上方（負座標）也不行
        let g = Geometry { x: 300, y: -900, width: 1100, height: 720 };
        assert!(!titlebar_is_reachable(&g, &[FHD]));
    }

    #[test]
    fn a_second_monitor_makes_its_own_coordinates_reachable_again() {
        let second = (1920, 0, 1920, 1080);
        let g = Geometry { x: 2400, y: 300, width: 1100, height: 720 };
        assert!(titlebar_is_reachable(&g, &[FHD, second]));
    }

    #[test]
    fn with_no_monitors_reported_nothing_is_reachable() {
        let g = Geometry { x: 300, y: 200, width: 1100, height: 720 };
        assert!(!titlebar_is_reachable(&g, &[]));
    }

    #[test]
    fn the_first_request_opens_a_window() {
        assert_eq!(decide(None, "acc-1", false), Decision::Open);
    }

    #[test]
    fn the_same_account_asking_again_focuses_the_open_window() {
        assert_eq!(decide(Some("acc-1"), "acc-1", true), Decision::Focus);
    }

    #[test]
    fn another_account_is_refused_while_the_window_is_open() {
        assert_eq!(decide(Some("acc-1"), "acc-2", true), Decision::Refuse);
    }

    #[test]
    fn another_account_may_open_once_the_window_is_gone() {
        // 歸屬還留著但視窗已經關掉（Destroyed 事件沒跑到也一樣要放行）
        assert_eq!(decide(Some("acc-1"), "acc-2", false), Decision::Open);
        assert_eq!(decide(Some("acc-1"), "acc-1", false), Decision::Open);
    }

    #[test]
    fn a_window_that_cannot_answer_is_not_there() {
        // Edge 更新收掉 WebView2 之後的幽靈：簿記還在，但問它什麼都問不到。
        assert!(!alive_from(None, None));
        assert!(!alive_from(None, Some(false)));
        assert!(!alive_from(Some(false), None));
        // 真的還開著，或只是被最小化。
        assert!(alive_from(Some(true), Some(false)));
        assert!(alive_from(Some(false), Some(true)));
        // 兩邊都說不在＝視窗被藏起來或已經沒了，一樣要放行。
        assert!(!alive_from(Some(false), Some(false)));
    }

    /// 每一組視窗都要拿到自己的 label。撞號＝新視窗建不出來（tauri 的簿記要等
    /// `Destroyed` 才清得掉舊 label），那正是 Edge 收掉 WebView2 之後的死法。
    #[test]
    fn each_generation_gets_a_label_of_its_own() {
        assert_ne!(toolbar_label(1), toolbar_label(2));
        assert!(toolbar_label(3).starts_with(TOOLBAR_LABEL_PREFIX));
    }

    /// 掃殘骸要連舊組的工具列一起認出來，又不能誤傷主視窗。
    #[test]
    fn the_sweep_recognises_toolbars_and_tabs_only() {
        assert!(is_browser_label(&toolbar_label(9)));
        assert!(is_browser_label(&tab_label(4)));
        assert!(!is_browser_label("main"));
    }

    /// 晚到的 `Destroyed` 只能清自己那一代。號碼對不上就代表下一組視窗已經接手，
    /// 清下去會把新視窗的 cookie 洗掉，開出來直接是未登入。
    #[test]
    fn a_late_teardown_only_clears_its_own_generation() {
        assert!(teardown_belongs_to(7, 7));
        assert!(!teardown_belongs_to(7, 8));
    }

    #[test]
    fn alias_is_escaped_into_the_shell_query_string() {
        assert_eq!(urlencode("abc-1_2.3~"), "abc-1_2.3~");
        assert_eq!(urlencode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(urlencode("倉庫"), "%E5%80%89%E5%BA%AB");
    }
}

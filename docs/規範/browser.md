# browser — 模組規範

> 本模組契約的唯一 owner。涵蓋 Rust 端 `browser` 模組與它專屬的工具列前端 `BrowserShell`。
> 最後更新：2026-08-18（重寫版：雙視窗貼合＋分頁）

## 架構（為什麼長這樣）

- **工具列視窗**（label `browser`，`WebviewWindow`）：自己的 webview，畫整圈邊框＋標題列（帳號名｜分頁框框｜＋）＋導覽列；transparent、上兩角圓下兩角方。
- **分頁視窗**（label `browser-tab-{id}`，每分頁一個 `WebviewWindow`）：owned window 疊在工具列視窗畫出的框裡（內縮 EDGE、工具列下方），切分頁＝顯示/隱藏。owner 關係讓它恆在工具列之上、跟著最小化與銷毀、不進工作列。
- **不用 multi-webview（`Window::add_child`）**：子 webview 在 Windows 收不到鍵盤（Chromium 視為未啟用而丟鍵，2026-08-18 診斷坐實，證據見 docs/瀏覽器重寫目標.md）。獨立視窗的 webview 與主視窗同構，鍵盤正常——本架構的成立前提就是這個。
- 不需要 tauri 的 `unstable` feature。

## 公開介面

- `browser::open(app, account_id, alias, jar)` — 開瀏覽器（守門→注 cookie→工具列視窗→第一個分頁）。
- `browser::navigate(app, action, url)` — `back`／`forward`／`reload`／`goto`（作用於**作用中分頁**）。
- `browser::tab_command(app, action, id)` — `new`／`activate`／`close`。
- command 包裝（`lib.rs`）：`open_account_browser(token, account_id, alias)`、`browser_navigate(action, url)`、`browser_tab(action, id)`。
- 事件（→ 工具列 webview）：`browser://tabs`＝`[{id,title,active}]`；`browser://nav`＝`{url}`（作用中分頁的網址）。
- 錯誤碼：`SESSION_EXPIRED`（無 session 或 jar 無 beanfun cookie）、`BROWSER_STILL_OPEN`（他帳號視窗組還開著）。
- 前端入口：`MainPage.vue` 的帳號頭像本身（hover 浮出地球）。
- 快捷鍵：**Ctrl+W 關作用中分頁、Ctrl+T 開新分頁**（`hook_tab_shortcuts`，掛在工具列與每顆分頁的 WebView2 `AcceleratorKeyPressed` 上——分頁零 IPC，只有這層攔得到）；**滑鼠中鍵點分頁框＝關那個分頁**（殼層 `@mousedown.middle`）。
- dev 測試帳號：debug 組建 `lib.rs` 塞假 jar（token `dev-test`）＋`App.vue` DEV 塞「測試帳號」列——正式打包版兩邊都不存在。

## 單一來源

- 視窗組歸屬：`WINDOW_OWNER`；守門判斷：`decide(owner, requested, alive)`。
- 分頁狀態（清單/作用中/流水號/cookie 快照）：`STATE: Mutex<BrowserState>`。前端不得自己記分頁清單，一律吃 `browser://tabs`。
- 版面幾何：`TITLEBAR_H`/`NAVBAR_H`/`EDGE` 由 Rust 定，開窗時放進殼層 query string；分頁視窗的實體位置由 `tab_rect_px` 算。**CSS 不得寫死這些尺寸**。
- 視窗幾何記憶：`Geometry`＋`browser-window.json`（實體像素、全帳號共用）；`titlebar_is_reachable` 判記住的位置還在不在螢幕上。
- 網址正規化：`normalize_url`——看得出是網址就開（裸主機補 https），看不出來的**轉 Google 搜尋**（`search_url`）；**永遠只回 http(s)**，`javascript:`/`file:` 會變成搜尋字串而不被執行。起始頁：`HOME_URL`。
- 登入態判定網域：`is_beanfun_domain`（只用來判斷 session 有沒有登入意義，**不是注入過濾條件**）。

## 不變量

- **cookie 必須在分頁導向目標之前注入完成**：分頁一律以 `about:blank` 建立，注入完才 `navigate`。每次手動開分頁都重注一次（注入便宜、TTL 短）。
- 注入的 cookie **一定要給到期時間**（`INJECTED_COOKIE_TTL_SECS`）——session cookie 只活在建立它的 webview，其他分頁與彈窗看不到。
- **注入不按網域過濾**，jar 有幾顆注幾顆（彈窗走 gamania 關聯網域，被過濾掉的正是那些）。單向注入，不回寫。
- 旗標一律照抄 jar，**唯一例外是 `bfWebToken` 要拿掉 HttpOnly**（`is_js_readable_cookie`）：SSO 檢查點 `tw.newlogin.beanfun.com/checkin_step2.aspx` 的 `DealWebToken()` 是用 `document.cookie` 讀這顆的，讀不到就 `GotoLoginPage()` 把使用者打回掃碼登入頁。詳見 docs/瀏覽器登入態診斷.md。要再加例外必須先有「哪段網頁 JS 讀它」的證據。
- 分頁視窗（`browser-tab-*`）**不得出現在任何 capability 檔**＝零 IPC——它載外部網站。
- 工具列視窗一動（Moved/Resized/ScaleFactorChanged）就 `relayout_tabs` 把**所有**分頁貼回框裡（隱藏中的也排，切換時才不閃舊位置）。
- 網頁要求的新視窗：**帶尺寸特徵（`features.size()` 有值）→ `Allow` 原生彈窗**（金流靠 `window.opener` 回報付款結果）；**沒帶尺寸 → `Create` 開成分頁**（走 `SetNewWindow`，opener 一樣保留）。分頁開不成要退回 `Allow`，不可吞掉連結。
- `Create` 的分頁 builder 必須套 `window_features(features)`（沿用來源 webview 的 WebView2 environment，`Create` 的硬性要求），且**不得自行 navigate**（內容由 WebView2 灌入）。
- 關掉最後一個分頁＝關掉整個瀏覽器，且要走 `toolbar.close()`（讓 `CloseRequested` 存幾何）；工具列 `Destroyed` 時補 destroy 所有分頁並清空 `STATE` 與 `WINDOW_OWNER`。
- 分頁網址的更新要掛 **`on_navigation`**（回 `true` 放行）＋`on_page_load(Finished)` 兩處：上一頁/下一頁這類歷史導航**不觸發 on_page_load**，只靠它網址列會停在舊網址（重寫當日實測踩到）。`about:blank` 不推進網址列。
- 殼層任何區塊都不能長高：工具列高度是 Rust 算好的。錯誤訊息蓋在網址列上。
- 視窗先 `visible(false)` 開、套完幾何才 `show()`；分頁先 `relayout` 再顯示。
- 邊框用 inset box-shadow 不用 CSS border；握把在 `.frame` 內靠 overflow:hidden 裁切；角落握把 14px 上限（不碰標題列按鈕）。`EDGE` 是「細」與「抓得住」的取捨點。
- 注進分頁的初始化腳本（`THIN_SCROLLBAR_SCRIPT`）只准動外觀。

## 禁止

- 不從 `browser` 讀 `AppState` 內部欄位——由 command 解出 jar 傳進來。
- 不在同步 command 裡建視窗——走 `async` command（WebView2 同步情境 deadlock）。
- 不在 WebView2 事件回呼裡**阻塞**執行緒（等 channel 問 `CanGoBack` 卡死過整個 app）。註：`on_new_window` 裡**建視窗**是安全的——wry 會把 handler 排進訊息迴圈（deferral）再跑；不安全的是「在回呼裡等另一個執行緒的答案」。
- 網址列不得成為任意 scheme 的入口（`normalize_url` 把關）。
- 網頁登出不攔截——交給既有 `ping_session`。
- cookie 注入／導覽／分頁動作的失敗不可靜默吞掉——stderr `[browser]` 前綴＋殼層顯示。
- 網址列編輯不掛 `blur` 取消——只有 Escape（焦點被分頁視窗搶走時 blur 會清掉打好的網址）。但**切換作用中分頁時殼層必須強制結束編輯**（tabs 事件裡比對 active id）——否則點過網址列再切分頁，網址列會停在舊分頁的網址。
- `AcceleratorKeyPressed` 回呼裡不得直接建/銷毀視窗（重入）——動作一律丟到另一條執行緒再呼叫 `close_tab`/`tab_command`。
- 拒絕他帳號時不代關既有視窗——回 `BROWSER_STILL_OPEN` 讓前端警告。

## 已知取捨

- 全 app 同時一組瀏覽器：WebView2 同 app 共用 cookie 儲存區，兩帳號同開會互踩登入態。（未來解法＝wry 0.56 的 `with_profile_name` 具名 profile，tauri 尚未曝露。）
- 帶尺寸的金流彈窗長 WebView2 原生樣，與 app 風格不同——換成自家分頁會有付款回報風險，使用者拍板保守處理。
- 守門只看工具列視窗；原生彈窗比它長壽時，開別的帳號會踩掉彈窗的登入態（彈窗通常短命，暫時接受）。
- 上下頁走手打 COM（`GoBack`/`GoForward`）；上下頁鈕一律可按（問 `CanGoBack` 要嘛阻塞要嘛加事件管線，先不做 disabled 狀態）。
- 拖動工具列時分頁靠 `Moved` 事件跟隨，理論上有一兩幀的延遲；實測貼合正確，手感待使用者驗收。

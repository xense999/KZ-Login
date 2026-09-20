# browser — 模組規範

> 本模組契約的唯一 owner。涵蓋 Rust 端 `browser` 模組與它專屬的工具列前端 `BrowserShell`。
> 最後更新：2026-09-09（工具列 label 改逐組換號；貼合與視窗尺寸改吃系統文字倍率）

## 架構（為什麼長這樣）

- **工具列視窗**（label `browser-shell-{組號}`，`WebviewWindow`）：自己的 webview，畫整圈邊框＋標題列（帳號名｜分頁框框｜＋）＋導覽列；transparent、上兩角圓下兩角方。
- **分頁視窗**（label `browser-tab-{id}`，每分頁一個 `WebviewWindow`）：owned window 疊在工具列視窗畫出的框裡（內縮 EDGE、工具列下方），切分頁＝顯示/隱藏。owner 關係讓它恆在工具列之上、跟著最小化與銷毀、不進工作列。
- **不用 multi-webview（`Window::add_child`）**：子 webview 在 Windows 收不到鍵盤（Chromium 視為未啟用而丟鍵，2026-08-18 診斷坐實，證據見 docs/瀏覽器重寫目標.md）。獨立視窗的 webview 與主視窗同構，鍵盤正常——本架構的成立前提就是這個。
- 不需要 tauri 的 `unstable` feature。

## 公開介面

- `browser::open(app, account_id, alias, jar)` — 開瀏覽器（守門→注 cookie→工具列視窗→第一個分頁）。
- `browser::navigate(app, action, url)` — `back`／`forward`／`reload`／`goto`（作用於**作用中分頁**）。
- `browser::tab_command(app, action, id)` — `new`／`activate`／`close`。
- `browser::read_cookies(window, url) -> Vec<SeenCookie>` — 從某顆 webview 讀出 `url` 適用的 cookie，**連同它的網域、路徑與旗標**。給 `gamapass` 用：那條登入的 token 只落在那顆 webview 裡。`SeenCookie::as_set_cookie()` 把它還原成「當初的 Set-Cookie 那一行＋發它的網址」，交給 `beanfun::adopt_cookies` 照原樣收進 jar。
- `browser::SESSION_COOKIE_URLS` — beanfun 的登入態散在哪三個網址底下。要清舊的（本模組、`gamapass`）、要撈新的（`gamapass`）都問這一份。
- `browser::seed_and_navigate(window, jar, stale, target)` — 先刪掉 `stale` 那幾個網址底下的舊 cookie，再把 jar 的 cookie 注入，最後才導向；三步靠回呼串成先後，不是各做各的；怎麼刪才不會傷到注入的那批，見下面「注入之前先清」。**不是整個清空**：同一個資料夾裡還住著別的網域的登入態。給 `gamapass` 的登入視窗用；cookie 注入的實作歸屬仍在本模組，不另開第二套。
- `browser::eval_json(window, script)` — 在某顆 webview 的頁面裡跑一段腳本、拿回結果（JSON 字串），問不到回 `None`。給 `gamapass` 用：那顆視窗零 IPC，頁面的狀態只能由後端去問。
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

- **cookie 的網域不可以丟。** 撈回來的 cookie 之後會再被注入別的 webview；注入只蓋得掉「名稱＋網域＋路徑」都相同的那一顆，所以掛錯網域的複本蓋不掉別人、也不會被正確的那一顆蓋掉，兩顆一起送出去，beanfun 就判成未登入。v2.4.0 的 `adopt_cookies` 把撈回來的全部掛到 `tw.beanfun.com`，從 GamaPass 卡片開過一次瀏覽器後，那批錯位的 `bfUID`／`bfSecretCode` 就留在共用資料夾裡，連之後 QR 登入的瀏覽器都開成未登入，安裝版與 dev 一起壞（2026-09-19 實機，靠印出儲存區內容才看出同名 cookie 各有兩份）。
- **注入之前先清。** cookie 儲存區是所有帳號、所有 session 共用的，要進來的那一批才是現在登入的人：
  - 換了帳號（跟上次開的不是同一個，或程式剛啟動不知道上次是誰）→ **整個清空**再注入（`wipe_cookies_then`）。beanfun 的登入 cookie 我們認得，但活動頁那類子網域會發自己的 session cookie，名字與網域都不在我們手上，留著就可能讓下一個帳號沿用上一個人的 session（2026-09-19 使用者定；代價是他在這個瀏覽器裡登過的第三方網站換帳號後要重登）。
  - 同一個帳號 → 只刪 `SESSION_COOKIE_URLS` 底下**同名卻掛在別處**的再注入，其他網站的登入不受影響。
  - **`DeleteCookie` 沒有完成通知，也不保證比後面的寫入早生效**，所以同一個帳號那條路三條一起守（`CookieSlots`、`clear_cookies_then`）：
    1. **注入要用的位置（名稱＋網域＋路徑）一律不刪**——注入本來就會蓋掉它；不刪，晚到的刪除就碰不到注入的東西。
    2. **同一顆 cookie 只送一次刪除**。`.beanfun.com` 的 cookie 在三個網址底下都讀得到，各送一次的話，第一個生效後看起來清乾淨了，其餘的還在路上。
    3. **送出刪除後要再讀，確認不在了才注入**；注入後再等一次讀取回來才導向。
    2026-09-20 實機：同一個帳號連開四次，一、三次有登入，二、四次沒有。沒登入的那兩次儲存區裡 `bfWebToken`／`bfUID`／`bfSecretCode` 整顆不見——上一次注入成功的那顆這次被排了刪除，刪除落在新注入之後。失敗的那次沒東西可刪，所以下一次又好了，一好一壞輪流（靠暫時把儲存區內容寫成檔案才看到）。
  - **整個清空用 `DeleteAllCookies`，但要確認空了才注入**：網頁內嵌框架（首頁的 YouTube 播放器）留下的分割 cookie，`DeleteCookie` 刪不到（實機：`.youtube.com` 那五顆讀幾十輪都還在，害「清乾淨」永遠不成立、每次開都變成整個清空）。它一樣沒有完成通知、會晚到，所以送出後一直讀到儲存區空了才往下走；**本來就是空的就不送**——沒有東西可以拿來確認它落地了沒，晚到的清空會把剛注入的帶走。同一個帳號那條路不可以用它：什麼都刪，沒辦法把注入的位置排除在外。
  - **只有每次開啟的第一個分頁會刪**；之後的「+」分頁只注入。瀏覽期間網站自己發的同名 cookie（別的子網域的 `ASP.NET_SessionId` 之類）是使用者正在走的流程，刪了等於把那個流程的 session 砍斷。
  - **儲存區的主人要等確認清空了才換**（`COOKIE_OWNER`）；沒清乾淨就登記成「不知道是誰的」，下次不管開誰都再清一次。在判斷的當下就登記的話，開到一半失敗、清空沒發生，下一次再開同一個帳號會被當成沒換帳號。
  - 已知的限制：主人認的是卡片 id。同一張卡片被重新登入成**另一個** beanfun 帳號（只會發生在卡片底下沒有遊戲帳號可比對的時候）不會觸發整個清空。

- **cookie 必須在分頁導向目標之前注入完成**：分頁一律以 `about:blank` 建立，注入完才 `navigate`。每次手動開分頁都重注一次（注入便宜、TTL 短）。
- 注入的 cookie **一定要給到期時間**（`INJECTED_COOKIE_TTL_SECS`）——session cookie 只活在建立它的 webview，其他分頁與彈窗看不到。
- **注入不按網域過濾**，jar 有幾顆注幾顆（彈窗走 gamania 關聯網域，被過濾掉的正是那些）。單向注入，不回寫。
- 旗標一律照抄 jar，**唯一例外是 `bfWebToken` 要拿掉 HttpOnly**（`is_js_readable_cookie`）：SSO 檢查點 `tw.newlogin.beanfun.com/checkin_step2.aspx` 的 `DealWebToken()` 是用 `document.cookie` 讀這顆的，讀不到就 `GotoLoginPage()` 把使用者打回掃碼登入頁。詳見 docs/瀏覽器登入態診斷.md。要再加例外必須先有「哪段網頁 JS 讀它」的證據。
- 分頁視窗（`browser-tab-*`）**不得出現在任何 capability 檔**＝零 IPC——它載外部網站。
- **工具列 label 每開一組換一個號碼**（`toolbar_label(generation)`），不得改回固定字串。`destroy()` 一律走 `proxy.send_event`（tauri-runtime-wry 2.11 的 `destroy` 明文不走 `send_user_message`），tauri 的 webview 簿記要等 event loop 收到 `Destroyed` 才清 label——「砍幽靈→立刻用同一個 label 建新視窗」**必定**報 `a webview with label ... already exists`。Edge 在背景更新收掉 WebView2 留下幽靈時走的就是這條路。capability 因此是 glob `browser-shell-*`（`capabilities/browser.json`）。
- 開新一組前掃殘骸靠 `is_browser_label`（舊工具列＋所有分頁），**不等它們消失**。
- 工具列視窗一動（Moved/Resized/ScaleFactorChanged）就 `relayout_tabs` 把**所有**分頁貼回框裡（隱藏中的也排，切換時才不閃舊位置）。
- **算貼合位置要用 `css_to_px(scale_factor())`，不是裸的 `scale_factor()`**：Windows 的「協助工具 → 文字大小」不進 tao 回報的 DPI，卻被 WebView2 併進整頁縮放，殼層畫出來的標題列因此比 `TOOLBAR_H * scale_factor()` 高——少算就讓分頁視窗往上蓋掉分頁列與網址列。
- **工具列的初始尺寸與下限走 `win::size_for_text_scale`**（與主視窗同一個函式，不另寫一份）：乘上倍率**並夾進工作區**。不夾的話 `DEFAULT_H` 在 225% 下算出 1620，比多數螢幕高，而工具列的標題列是自繪的，視窗一超出螢幕就抓不到、關不掉。記住的幾何是實體像素、已含當時的設定，直接套、不再乘一次。
- 網頁要求的新視窗：**帶尺寸特徵（`features.size()` 有值）→ `Allow` 原生彈窗**（金流靠 `window.opener` 回報付款結果）；**沒帶尺寸 → `Create` 開成分頁**（走 `SetNewWindow`，opener 一樣保留）。分頁開不成要退回 `Allow`，不可吞掉連結。
- `Create` 的分頁 builder 必須套 `window_features(features)`（沿用來源 webview 的 WebView2 environment，`Create` 的硬性要求），且**不得自行 navigate**（內容由 WebView2 灌入）。
- 關掉最後一個分頁＝關掉整個瀏覽器，且要走 `toolbar.close()`（讓 `CloseRequested` 存幾何）；工具列 `Destroyed` 時補 destroy 所有分頁並清空 `STATE` 與 `WINDOW_OWNER`。**例外＝先關主視窗**：那條路走 `app.exit(0)`，不觸發任何視窗的 `CloseRequested`，幾何不會被存（見已知取捨）。
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
- **先關主視窗＝不存瀏覽器幾何**：主視窗的 `CloseRequested` 走 `app.exit(0)` 結束整個程式（沒有系統匣也沒有 single-instance，主視窗一關就叫不回來，而幽靈視窗條目會讓進程連退都退不掉）。`exit` 直接停掉 event loop，不觸發任何視窗的 `CloseRequested`，所以這條路存不到 `browser-window.json`。使用者 2026-09-09 拍板接受（另一案「先存幾何再 exit」當場否決）。
- **改完系統文字大小要重開程式**：倍率只在啟動時讀一次（`win::text_scale_factor` 用 `OnceLock` 快取，否則拖動時每個 `Moved` 都要開一次登錄檔）。改完設定直接開瀏覽器，`load_geometry` 套的會是**舊倍率**下記住的尺寸，可能偏大或偏小，手動拉一次即可。

# gamapass — GamaPass 登入視窗

歸屬見 [模組歸屬總表](../規範.md) 的 `gamapass` 條。

規格書：[#9](https://github.com/xense999/KZ-Login/issues/9)（2026-09-17 建）

## 公開介面

```rust
pub struct Fill { account: String, password: Option<String> }
pub enum Outcome { Completed { token, cookies }, Cancelled }
pub async fn wait_for_login(app, skey: &str, jar, fill: Fill) -> Result<Outcome, String>
pub fn cancel(app)
```

- 呼叫者：`commands` 的 `gamapass_login`、`gamapass_cancel`（登入頁的「取消」）。
- `Completed` 代表視窗的 cookie 裡出現了 `bfWebToken`，token 與整批 cookie 一起帶回來；呼叫端用 `beanfun::adopt_cookies` 收進自己的 jar。
- 視窗開在 beanfun 的登入頁（`Login/Index?pSKey=…`），GamaPass 的入口網址由**那個頁面自己**去要，本模組不認得它長什麼樣。
- `password: None` ＝ passkey：帳號照填、那一步照過，然後把視窗交給使用者。

## 畫面

登入頁自己問，一頁問完：手機號碼或電子郵件、密碼，一顆「登入」。對方的頁面是分兩步問的，我們不必跟著分。

- 記住的帳密（見總表 `credentials` 條）：最後用的那組進來就填好，其他的收在帳號欄右邊的 ▾ 裡，每列可點選帶入、可按 ✕ 刪除——跟帳密登入頁同一套（差別是沒有拖曳排序）。
- 「改用 passkey（不用密碼）」要填了帳號才能按——不然沒有東西帶得進去，使用者得在對方頁面重打一次。
- **我們的表單只是預先打**：按下登入後，登入視窗會跳出來（480×760、置中於螢幕），腳本在使用者眼前把填的東西打進對方的欄位。兩條路都一樣。
- **視窗是兩個視窗**（同帳號瀏覽器的架構）：外殼 `gamapass-shell-<n>` 畫邊框與標題列（`GamaPassShell.vue`／`gamapass.html`），網頁那顆 `gamapass-view-<n>` 貼在框裡。對方的頁面裡畫不了我們的標題列，而原生外框會讓它看起來像另一個程式。版面常數（`TITLEBAR_H`、`EDGE`）兩邊各一份，改了要一起改。
- 腳本填不動的（二階段、passkey、密碼錯了）就在同一個視窗裡自己接手——它本來就開著，沒有東西要現身、也沒有蓋子要掀。視窗上方那行字會說程式走到哪一步、為什麼停下來。
  - ★ 2026-09-17 試過「藏起來只在需要時現身」與「貼在主視窗內容區＋蓋住對方頁面」，都有頁面問題；passkey 更麻煩：Windows 只肯替在最前面的視窗跳出指紋／PIN 的框。定案就是單純開一個看得見的視窗。

## 流程

1. `gamapass_login`：建一個新的 client + cookie jar，`get_session_key` 拿 `pSKey`，`open_login_page` 建立那把 key 的登入頁。
2. 開一個隱藏的 WebView 視窗，**先停在 `about:blank`、把 client 的 cookie 注入進去（`browser::seed_and_navigate`，注入前會先清空）**，再導向 beanfun 的登入頁。注入的腳本在那裡按下「使用 gamapass」，到了對方網域再把帳密填進欄位並送出。
3. 每一輪去讀那個視窗的 cookie（三個 beanfun 網域）：
   - 出現 `bfWebToken` ＝ 登入完成 → 整批 cookie 收進我們的 jar，接著 `get_game_accounts`，登記進 `session_stores`。
   - 視窗被關掉或 10 分鐘沒結果 ＝ 取消。

**為什麼要去 webview 裡撈 cookie**（2026-09-17 實機推翻先前的設計）：QR 登入可以由我們的 client 收尾，因為那條的登入態綁在 `pSKey` 上；**GamaPass 不是**——它把 `bfWebToken` 發給「執行登入的那個瀏覽器」，`complete_login` 在我們的 client 上跑只會得到「任何 cookie 裡都找不到 bfWebToken」。那顆 cookie 同時也是唯一可靠的成功信號：登入成不成功，頁面都會回到 beanfun。

## 單一來源

- **入口網址一律由那個視窗自己去要**（按下 beanfun 登入頁的「使用 gamapass」，由頁面呼叫 `Login/GoGamaPass`）。beanfun 把 OAuth 的 nonce 綁在「提出請求的那條 session」上，我們用 Rust 的 client 代打、再把網址交給視窗，繞回來就是 `AUCB001 參數(nonce)驗證失敗`——即使 cookie 已經複製過去也一樣。要那個網址、跳到對方網域、繞回來，必須是同一個 browser context。寫死 `accounts.gamania.com/login` 更不行：那樣登完會停在橘子那邊，沒有東西回到 portal。
- **登入完成的判定**只寫在本模組的 `harvest`：視窗的 cookie 裡有 `bfWebToken` 才算完成。看網址不算數——失敗也會回到 beanfun。
- **視窗的樣子與位置**只寫在本模組（480×760、置中於螢幕；版面常數 `TITLEBAR_H`／`EDGE` 與 `GamaPassShell.vue` 各一份，改了要一起改），前端不傳尺寸也不傳配色。

## 不變量

- 密碼只在這一次登入的過程中存在：後端只轉交給視窗，成功時才交給 `credentials` 記住（kind = gamapass），卡片的 `loginAccount` 是 null。
- 注入的腳本在 `login.beanfun.com` 與 `accounts.gamania.com` 兩個 host 上跑（前者只按那顆「使用 gamapass」），**但帳密只會填進 `accounts.gamania.com`**——別的頁面碰巧載進這個視窗時，一個字都不會被打出去。
- 腳本只做「按 beanfun 頁上的 gamapass、填欄位、按下一步、按登入」。不偽裝自動化痕跡、不碰任何驗證挑戰；對方要驗就讓它跳出來給使用者做。
- 欄位靠 `input` 的 type 找、按鈕靠文字找，不用對方的 class：那是框架產生的名字，改版就會變。
- 網頁那顆視窗**不掛任何 capability**（零 IPC），同 `captcha`：載入的是外部網站，給它 IPC 等於把 app 的指令開放給那個頁面。
- **第一個真正的請求之前 cookie 就要就位**：登入態是 beanfun 綁在這條 session 上的，webview 沒帶著同一批 cookie，OAuth 繞回來時 beanfun 認不得自己發的 nonce，回「參數(nonce)驗證失敗」。注入前先清空，否則上一次登入留下的 token 會讓 portal 短路掉這一次。
- 使用固定、可重用的獨立 WebView2 資料夾（app local data 底下的 `gamapass-webview`），不可與主視窗或 captcha 視窗共用。
- label 每次換號（`gamapass-shell-<n>`／`gamapass-view-<n>`）：tauri 的 label 簿記要等 `Destroyed` 才清，用固定 label 會撞號。
- **只有外殼掛 capability**（拖曳與關閉），網頁那顆零 IPC。
- **焦點給網頁那顆，不是外殼**：Windows 的安全性驗證（指紋／PIN）會掛在發起它的視窗上，那顆沒有焦點的話，框會冒在別人後面。
- **每一輪都呼叫 `AllowSetForegroundWindow(ASFW_ANY)`**：那個框是系統自己的程序畫的，Windows 的前景鎖不讓別的程序搶到最前面，少了這個授權它只會在工作列閃、要使用者自己點。★**不要用置頂代替**：那個框不是掛在我們視窗底下的東西，置頂只會蓋住它（2026-09-17 實機踩過）。
- 關掉視窗就是取消；10 分鐘逾時。

## 禁止

- 自己實作 GamaPass 的登入 API 或 passkey —— 正面做法：登入請求要帶對方頁面才產得出的 reCAPTCHA v3 token，passkey 的憑證又綁在對方網域（WebAuthn 的 RP ID），兩者都只能在他們的頁面上完成；我們負責的是把畫面與輸入接過來。
- 拿 `complete_login` 來收尾這條登入 —— 正面做法：token 在那個視窗的 cookie 裡，撈出來收進 jar。`complete_login` 是 QR 那條的收尾，在這裡只會失敗（而且它會 POST `return.aspx`，不是唯讀）。

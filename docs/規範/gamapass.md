# gamapass — 在應用程式內完成的 GamaPass 登入

歸屬見 [模組歸屬總表](../規範.md) 的 `gamapass` 條。

規格書：[#9](https://github.com/xense999/KZ-Login/issues/9)（2026-09-17 建；2026-09-19 改為「看不見的視窗＋記住裝置」，passkey 拔除）

## 公開介面

```rust
pub struct Fill { account: String, password: String }
pub enum Stage { Working, Code { sent_to, error, attempt }, User }
pub enum Outcome { Completed { token, cookies }, Cancelled }
pub async fn wait_for_login(app, skey: &str, jar, fill: Fill, region: overlay::Region, on_stage: impl Fn(&Stage)) -> Result<Outcome, String>
pub fn submit_code(app, code: &str) -> Result<(), String>
pub fn cancel(app)
```

- 呼叫者：`commands` 的 `gamapass_login`、`gamapass_code`、`gamapass_cancel`（登入頁的「取消」）。
- `Completed` 代表視窗的 cookie 裡出現了 `bfWebToken`，token 與整批 cookie 一起帶回來；呼叫端用 `beanfun::adopt_cookies` 收進自己的 jar。
- `on_stage` 在 `Stage` 每次改變時被叫到；怎麼送到前端（事件名）是 `commands` 的事，本模組不認得。
- `region` 是視窗需要現身時要貼的那一塊（主視窗客戶區的 CSS px），由前端量測後傳入。

## 為什麼這樣做：橘子會記住裝置（2026-09-19 在 Edge 實機確認＋讀對方原始碼）

- **只有「密碼登入」會被記住。** 對方的密碼登入會把「保持登入狀態」的勾選值一起送出（`rememberLoginStatus`）；passkey 登入完全不帶，passkey 替代驗證（驗證碼）那條實測也不會被記住。
- 被記住之後，下次進來落在 `/login/select-account`：列出記住的帳號（遮罩過的手機／信箱），點一下就登入——不問密碼、不問驗證碼、沒有 Windows 安全性的框。最多記 5 組；過期的會退回一般登入。
- 登入態在對方伺服器上，瀏覽器這邊只有 `accounts.gamania.com` 的 cookie（長效，重開瀏覽器仍在）。所以這顆視窗的資料夾就是「這台裝置」。
- **密碼登入成功後對方會推銷 passkey**（「啟用 Passkey (免密碼登入)」／「Passkey 已設定，要設為預設登入嗎?」）。答應了，這個帳號以後按「下一步」就直接走 passkey，再也到不了密碼頁，也就再也不會被記住。
- beanfun 按登出**不會**洗掉對方記住的帳號。

## 畫面

`GamaPassPage` 分兩塊（版面 A，2026-09-19 使用者定）：

- **已記住的帳號**：下拉選單＋「登入」。清單來自我們自己的 `credentials`（kind = gamapass）——在這裡用密碼登入成功過的帳號，對方那邊也就記住了。每列可按 ✕ 刪除。沒有記住任何帳號時這一塊不出現。
- **新增帳號**：手機號碼或電子郵件、密碼、「新增並登入」。登入成功才記進清單。
- 沒有 passkey 的入口（2026-09-19 使用者定：直接拔掉）。

登入期間頁面跟著 `gamapass-stage` 事件走：

- `working`：「登入中…」。記住的帳號通常幾秒內就結束，使用者只看到這個。
- `code`：我們自己的驗證碼輸入框（四位數，打滿自動送出）。對方不收時 `error` 帶著對方頁面上的說法再問一次。
- `user`：登入視窗貼到這一頁的內容區上（標題列與底部按鈕列之間），對方的頁面原樣交給使用者。底部的「取消」仍然有效。

## 流程

1. `gamapass_login`：建一個新的 client + cookie jar，`get_session_key` 拿 `pSKey`，`open_login_page` 建立那把 key 的登入頁。
2. 開一顆**停在畫面外**的 WebView 視窗，先停在 `about:blank`，由 `browser::seed_and_navigate` **只清 beanfun 三個網域的舊 cookie**、注入 client 的 cookie，再導向 beanfun 的登入頁。
3. 注入的腳本每 300ms 看一次頁面在哪裡、做那一頁該做的一件事：
   - beanfun 登入頁：按「使用 gamapass」。
   - `/login/select-account`：點遮罩對得上的那一列；對不上或過期就按「使用其他帳號登入」。
   - `/login`：填帳號、勾「保持登入狀態」、按「下一步」。
   - `/login/input-password`：填密碼、按「登入」（只送一次）。
   - 任何一頁出現驗證碼輸入框（`.input-verification-code`）：回報 `code`，等 `submit_code`。
   - passkey 的推銷框：按「繼續使用密碼」／「稍後再說」。**放手之後也照按。**
   - 圖形驗證跳出來、同一頁卡太久、沒見過的頁面：回報 `user`，從此不再動手。
4. 後端每 600ms 讀一次那顆視窗的 cookie（三個 beanfun 網域）與腳本的回報（`browser::eval_json`）：
   - 出現 `bfWebToken` ＝ 登入完成 → 整批 cookie 收進我們的 jar，接著 `get_game_accounts`，登記進 `session_stores`，記住帳密。
   - `Stage` 變了 → `on_stage`；變成 `User` 時把視窗貼到 `region` 上並給它焦點，之後每個 tick 重貼（跟著主視窗移動）。
   - 視窗被 `cancel` 或 10 分鐘沒結果 ＝ 取消。

**為什麼要去 webview 裡撈 cookie**（2026-09-17 實機推翻先前的設計）：QR 登入可以由我們的 client 收尾，因為那條的登入態綁在 `pSKey` 上；**GamaPass 不是**——它把 `bfWebToken` 發給「執行登入的那個瀏覽器」，`complete_login` 在我們的 client 上跑只會得到「任何 cookie 裡都找不到 bfWebToken」。那顆 cookie 同時也是唯一可靠的成功信號：登入成不成功，頁面都會回到 beanfun。

## 單一來源

- **入口網址一律由那個視窗自己去要**（按下 beanfun 登入頁的「使用 gamapass」，由頁面呼叫 `Login/GoGamaPass`）。beanfun 把 OAuth 的 nonce 綁在「提出請求的那條 session」上，我們用 Rust 的 client 代打、再把網址交給視窗，繞回來就是 `AUCB001 參數(nonce)驗證失敗`——即使 cookie 已經複製過去也一樣。寫死 `accounts.gamania.com/login` 更不行：那樣登完會停在橘子那邊，沒有東西回到 portal。
- **登入完成的判定**只寫在本模組的 `harvest`：視窗的 cookie 裡有 `bfWebToken` 才算完成。看網址不算數——失敗也會回到 beanfun。
- **要清哪些 cookie** 只寫在本模組的 `STALE_COOKIE_URLS`；怎麼清、怎麼排在注入之前，屬於 `browser::seed_and_navigate`（見總表 `browser` 條）。
- **視窗貼在哪裡**由前端量測後傳入（`overlay::Region`），貼上去的動作屬於 `overlay`；本模組不寫死任何版面尺寸。

## 不變量

- **`accounts.gamania.com` 的 cookie 永遠不清。** 那是對方記住這台裝置的依據；清了，每次登入都得從頭驗一遍。要清的只有 beanfun 的（上次留下的 token 會讓 portal 短路掉這一次），而且要**刪完才注入、注入完才導向**——各做各的話，晚到的刪除會把剛注入的那批一起帶走。
- **一律走密碼、一律勾「保持登入狀態」、一律婉拒 passkey。** 腳本把 `navigator.credentials.get/create` 換成直接回 `NotAllowedError`（等同使用者在系統的框上按取消）：passkey 登入不會被記住，而且視窗在畫面外，系統的框會憑空冒出來。開了 passkey 優先的帳號會因此走到對方的替代驗證，那一頁交給使用者（`user`），並在畫面上說明原因。
- **視窗是「顯示中但停在畫面外」，不是隱藏。** 隱藏的 WebView 不繪製，對方頁面的轉場不會結束、對話框不會打開。另外帶 `CalculateNativeWinOcclusion` 等參數，免得 Chromium 把畫面外的視窗當成被遮住而降速。
- **放手之後不收回。** 回報過 `user` 之後，後端不再理會腳本的其他回報，視窗一直留在畫面上到登入結束——頁面已經交給使用者，中途藏起來只會讓他打到一半的東西憑空消失。
- 密碼只在這一次登入的過程中存在：後端只轉交給視窗，成功時才交給 `credentials` 記住（kind = gamapass），卡片的 `loginAccount` 是 null。
- 注入的腳本在 `login.beanfun.com` 與 `accounts.gamania.com` 兩個 host 上跑（前者只按那顆「使用 gamapass」），**但帳密只會填進 `accounts.gamania.com`**——別的頁面碰巧載進這個視窗時，一個字都不會被打出去。
- 腳本不偽裝自動化痕跡、不碰任何驗證挑戰；對方要驗就交給使用者。
- 欄位靠 `input` 的 type 找、按鈕靠文字找；用到的少數選擇器是對方原始碼裡手寫的穩定名字（`.use-gama-pass`、`.input-verification-code`、ARIA role），不用框架產生的 class——那種名字改版就會變。
- 選帳號時，遮罩對得上的列**恰好一列**才點；兩列都對得上就不猜，改走密碼。
- 視窗**不掛任何 capability**（零 IPC），同 `captcha`：載入的是外部網站，給它 IPC 等於把 app 的指令開放給那個頁面。腳本的回報由後端主動去問（`browser::eval_json`），驗證碼由後端 `eval` 進去。
- 使用固定、可重用的獨立 WebView2 資料夾（app local data 底下的 `gamapass-webview`），不可與主視窗或 captcha 視窗共用。**刪掉這個資料夾＝對方忘記這台裝置。**
- label 每次換號（`gamapass-<n>`）：tauri 的 label 簿記要等 `Destroyed` 才清，用固定 label 會撞號。
- 10 分鐘逾時；成功的那條路由呼叫端關視窗（收尾失敗時那個畫面是唯一的線索）。

## 禁止

- 自己實作 GamaPass 的登入 API —— 正面做法：登入請求要帶對方頁面才產得出的 reCAPTCHA v3 token，只能在他們的頁面上完成；我們負責的是把輸入接過來。
- 提供 passkey 登入 —— 正面做法：passkey 每次都要過 Windows 驗證、而且不會被對方記住，與這個模組要達成的事相反。只有 passkey 的帳號請使用者到 gamapass 會員中心設定密碼並關閉「優先使用 Passkey」。
- 拿 `complete_login` 來收尾這條登入 —— 正面做法：token 在那個視窗的 cookie 裡，撈出來收進 jar。`complete_login` 是 QR 那條的收尾，在這裡只會失敗（而且它會 POST `return.aspx`，不是唯讀）。
- 登入前 `DeleteAllCookies` —— 正面做法：只清 `STALE_COOKIE_URLS`（v2.4.0 就是整個清空，所以每次都得重新驗證）。

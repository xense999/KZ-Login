# gamapass — GamaPass 登入視窗

歸屬見 [模組歸屬總表](../規範.md) 的 `gamapass` 條。

規格書：[#9](https://github.com/xense999/KZ-Login/issues/9)（2026-09-17 建）

## 公開介面

```rust
pub struct Fill { account: String, password: Option<String> }
pub enum Outcome { Completed, Cancelled }
pub async fn wait_for_login(app, entry_url: &str, fill: Fill) -> Result<Outcome, String>
pub fn cancel(app)
```

- 呼叫者：`commands` 的 `gamapass_login`、`gamapass_cancel`（登入頁的「取消」）。
- `Completed` 只代表「頁面已經回到 portal」，token 由呼叫端用 `beanfun::complete_login` 取得。
- `entry_url` 由 `beanfun::go_gamapass` 取得，本模組不認得 GamaPass 的網址長什麼樣。
- `password: None` ＝ passkey：帳號照填、那一步照過，然後把視窗交給使用者。

## 畫面

登入頁自己問，一頁問完：手機號碼或電子郵件、密碼，一顆「登入」。對方的頁面是分兩步問的，我們不必跟著分。

- 上次成功登入記住的那組（見總表 `credentials` 條）進來就填好，按登入即可；「忘記這組帳密」只在有記住時出現。
- 「改用 passkey（不用密碼）」要填了帳號才能按——不然沒有東西帶得進去，使用者得在對方頁面重打一次。
- **主視窗裡從頭到尾不放對方的頁面。** 自動填入期間那個視窗是隱藏的；需要人接手時它變成一個獨立視窗（480×720、置中於主視窗、進工作列）。

## 流程

1. `gamapass_login`：建一個新的 client + cookie jar，`get_session_key` 拿 `pSKey`，`open_login_page` 建立那把 key 的登入頁，`go_gamapass` 取得入口網址。
2. 開一個隱藏的 WebView 視窗載入入口網址，注入的腳本把帳密填進對方的欄位並送出。
3. 輪詢那個視窗的網址：
   - 回到 beanfun portal ＝ 登入完成 → `complete_login(client, store, skey)` 拿 `bfWebToken`，接著 `get_game_accounts`，登記進 `session_stores`。
   - fragment 出現 `kz-gamapass=user` ＝ 腳本請求把畫面交給人 → 顯示成獨立視窗。
   - 視窗被關掉或 10 分鐘沒結果 ＝ 取消。

**為什麼不用去 WebView 裡撈 cookie**：登入態綁在 `pSKey` 上而不是某一方的 cookie。QR 登入就是這樣——登入動作發生在手機上，我們的 client 全程沒送過帳密，`complete_login` 照樣拿得到 token。GamaPass 只是把「手機」換成「同一台電腦上的另一個視窗」。

## 單一來源

- **入口網址**只能來自 `Login/GoGamaPass`（`beanfun::go_gamapass`）。那個網址是 beanfun 按 session 產生的，帶著回到這把 `pSKey` 的路；寫死 `accounts.gamania.com/login` 會讓使用者登完停在橘子那邊，沒有東西回到 portal，`complete_login` 也就無從收尾。（那支 GET 也要帶防偽 token，漏了回「參數驗證失敗」。）
- **登入完成的判定**只寫在本模組的 `PORTAL_HOSTS`：網址的 host 落在 beanfun portal 才算完成。
- **視窗什麼時候現身**只寫在本模組：腳本用 fragment 求救時才現身。前端不控制這件事。

## 不變量

- 密碼只在這一次登入的過程中存在：後端只轉交給視窗，成功時才交給 `credentials` 記住（kind = gamapass），卡片的 `loginAccount` 是 null。
- 注入的腳本**只在 `accounts.gamania.com` 上作用**——帳密不能交給剛好載入這個視窗的任何其他頁面。
- 腳本只做「填欄位、按下一步、按登入」。不偽裝自動化痕跡、不碰任何驗證挑戰；對方要驗就讓它跳出來給使用者做。
- 欄位靠 `input` 的 type 找、按鈕靠文字找，不用對方的 class：那是框架產生的名字，改版就會變。
- 視窗**不掛任何 capability**（零 IPC），同 `captcha`：載入的是外部網站，給它 IPC 等於把 app 的指令開放給那個頁面。結果一律靠輪詢視窗網址取得。
- 使用固定、可重用的獨立 WebView2 資料夾（app local data 底下的 `gamapass-webview`），不可與主視窗或 captcha 視窗共用。
- label 每次換號（`gamapass-<n>`）：tauri 的 label 簿記要等 `Destroyed` 才清，用固定 label 會撞號。
- 關掉視窗就是取消；10 分鐘逾時。

## 禁止

- 自己實作 GamaPass 的登入 API 或 passkey —— 正面做法：登入請求要帶對方頁面才產得出的 reCAPTCHA v3 token，passkey 的憑證又綁在對方網域（WebAuthn 的 RP ID），兩者都只能在他們的頁面上完成；我們負責的是把畫面與輸入接過來。
- 從視窗裡把 cookie 撈出來當登入結果 —— 正面做法：登入態綁在 `pSKey` 上，用當初鑄出這把 key 的 client 走 `complete_login`，跟 QR 收尾同一條路。
- 反覆呼叫 `complete_login` 來試探有沒有登入成功 —— 它會 POST `return.aspx`，不是唯讀；只在判定完成後呼叫一次。

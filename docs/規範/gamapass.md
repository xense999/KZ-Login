# gamapass — GamaPass 登入視窗

歸屬見 [模組歸屬總表](../規範.md) 的 `gamapass` 條。

規格書：[#9](https://github.com/xense999/KZ-Login/issues/9)（2026-09-17 建）

## 公開介面

```rust
pub enum Outcome { Completed, Cancelled }
pub enum Mode { Autofill { account, password }, Manual }
pub async fn wait_for_login(app, entry_url: &str, mode: Mode) -> Result<Outcome, String>
pub fn cancel(app)
```

- 呼叫者：`commands` 的 `gamapass_login`、`gamapass_cancel`（登入頁的「取消」）。
- `Completed` 只代表「頁面已經回到 portal」，token 由呼叫端用 `beanfun::complete_login` 取得。
- `entry_url` 由 `beanfun::go_gamapass` 取得，本模組不認得 GamaPass 的網址長什麼樣。

## 單一來源

- **登入完成的判定**只寫在本模組的 `PORTAL_HOSTS`：網址的 host 落在 beanfun portal 才算完成。
- **視窗什麼時候現身**只寫在本模組：`Mode::Manual` 一開始就現身，`Mode::Autofill` 只在 fragment 求救時現身。前端不控制這件事。

## 單一來源（續）

- **入口網址**只能來自 `Login/GoGamaPass`（`beanfun::go_gamapass`）。那個網址是 beanfun 按 session 產生的，帶著回到這把 `pSKey` 的路；寫死 `accounts.gamania.com/login` 會讓使用者登完停在橘子那邊，沒有東西回到 portal，`complete_login` 也就無從收尾。

## 不變量

- 密碼只在這一次登入的過程中存在：前端送出後就清掉，後端只轉交給視窗，**不寫進 `credentials`**，卡片的 `loginAccount` 也是 null。
- 注入的腳本**只在 `accounts.gamania.com` 上作用**——帳密不能交給剛好載入這個視窗的任何其他頁面。
- 腳本只做「填欄位、按下一步、按登入」。不偽裝自動化痕跡、不碰任何驗證挑戰；對方要驗就讓它跳出來給使用者做。
- 欄位靠 `input` 的 type 找、按鈕靠文字找，不用對方的 class：那是框架產生的名字，改版就會變。
- 視窗**不掛任何 capability**（零 IPC），同 `captcha`：載入的是外部網站，給它 IPC 等於把 app 的指令開放給那個頁面。結果一律靠輪詢視窗網址取得。
- 使用固定、可重用的獨立 WebView2 資料夾（app local data 底下的 `gamapass-webview`），不可與主視窗或 captcha 視窗共用。
- 自動填入期間視窗隱藏且不進工作列；一旦現身就變成普通視窗（可關、在工作列看得到）。
- **進到 GamaPass 子頁就開視窗**，沒有「先按一顆按鈕」那一步——選了這個登入方式就是要登入。切走或按「取消」都會關掉它。
- label 每次換號（`gamapass-<n>`）：tauri 的 label 簿記要等 `Destroyed` 才清，用固定 label 會撞號。
- 關掉視窗就是取消，沒有第二種取消方式；10 分鐘逾時。

## 禁止

- 自己實作 GamaPass 的登入 API 或 passkey —— 正面做法：登入請求要帶對方頁面才產得出的 reCAPTCHA v3 token，passkey 的憑證又綁在對方網域，兩者都只能在他們的頁面上完成；我們負責的是把畫面與輸入接過來。
- 從視窗裡把 cookie 撈出來當登入結果 —— 正面做法：登入態綁在 `pSKey` 上，用當初鑄出這把 key 的 client 走 `complete_login`，跟 QR 收尾同一條路。
- 反覆呼叫 `complete_login` 來試探有沒有登入成功 —— 它會 POST `return.aspx`，不是唯讀；只在判定完成後呼叫一次。

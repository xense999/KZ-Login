# gamapass — GamaPass 登入視窗

歸屬見 [模組歸屬總表](../規範.md) 的 `gamapass` 條。

規格書：[#9](https://github.com/xense999/KZ-Login/issues/9)（2026-09-17 建）

## 公開介面

```rust
pub enum Outcome { Completed, Cancelled }
pub async fn wait_for_login(app, skey: &str) -> Result<Outcome, String>
pub fn cancel(app)
```

- 呼叫者：`commands` 的 `gamapass_login`、`gamapass_cancel`（登入頁的「取消」）。
- `Completed` 只代表「頁面已經回到 portal」，token 由呼叫端用 `beanfun::complete_login` 取得。

## 單一來源

- **登入完成的判定**只寫在本模組的 `PORTAL_HOSTS`：網址的 host 落在 beanfun portal 才算完成。
- **視窗的樣子與位置**（480×720、置中於主視窗）只寫在這裡，前端不傳任何尺寸。

## 不變量

- 帳密與二階段驗證都在遊戲橘子自己的頁面上，本程式不經手、不儲存，卡片的 `loginAccount` 是 null。
- 視窗**不掛任何 capability**（零 IPC），同 `captcha`：載入的是外部網站，給它 IPC 等於把 app 的指令開放給那個頁面。結果一律靠輪詢視窗網址取得。
- 使用固定、可重用的獨立 WebView2 資料夾（app local data 底下的 `gamapass-webview`），不可與主視窗或 captcha 視窗共用。
- label 每次換號（`gamapass-<n>`）：tauri 的 label 簿記要等 `Destroyed` 才清，用固定 label 會撞號。
- 關掉視窗就是取消，沒有第二種取消方式；10 分鐘逾時。

## 禁止

- 從視窗裡把 cookie 撈出來當登入結果 —— 正面做法：登入態綁在 `pSKey` 上，用當初鑄出這把 key 的 client 走 `complete_login`，跟 QR 收尾同一條路。
- 反覆呼叫 `complete_login` 來試探有沒有登入成功 —— 它會 POST `return.aspx`，不是唯讀；只在判定完成後呼叫一次。

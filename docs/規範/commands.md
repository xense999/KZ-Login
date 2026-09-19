# commands — Tauri 指令與 AppState

歸屬見 [模組歸屬總表](../規範.md) 的 `commands` 條。

（2026-09-14 首次建檔，隨 [#8](https://github.com/xense999/KZ-Login/issues/8)；以下只記登入相關指令，其餘待動到時補。）

## 公開介面（登入）

- `qr_start` / `qr_check`
- `password_login_start(account, password)` / `password_login_resume(captcha)` → `{ status: "approved", token, games } | { status: "captcha" } | { status: "rejected", message } | { status: "use_qr", message }`；網路錯誤走 `Err(String)`。
- `saved_logins() -> { account, password }[]`、`forget_saved_login(account)`、`reorder_saved_logins(accounts)`：記住的帳密（見總表 `credentials` 條）。
- `captcha_solve(palette, region) -> string | null`、`captcha_cancel()`：替暫停中的帳密登入開驗證視窗（site key 與頁面網址從暫停狀態取，前端不經手）。
- `gamapass_login(account, password, fresh, region) -> { status: "approved", token, games } | { status: "cancelled" }`、`gamapass_code(code)`、`gamapass_cancel()`：GamaPass 帳號走遊戲橘子自己的登入頁，在一顆看不見的視窗裡進行（見總表 `gamapass` 條）。指令會一直 await 到登入完成或取消；期間的進度以 **`gamapass-stage` 事件**送給主視窗（`{ stage: "working" } | { stage: "code", sentTo, error, attempt } | { stage: "user" }`），事件名只寫在 `GAMAPASS_STAGE_EVENT`。`fresh`＝這是新增的帳號（密碼未經對方驗證，不走選帳號的捷徑）；`region` 是那顆視窗需要現身時要貼的位置，由登入頁量測。**只有對方驗過的密碼才會被記住**（`password_checked`，見總表 `gamapass` 條）。指令第一步就要拿 `gamapass::ticket()`，取消才追得上還沒開窗的登入。
- `saved_gamapass() -> SavedLogin[]`、`forget_gamapass(account)`：GamaPass 記住的帳密，與 `saved_logins` 各自分流（見總表 `credentials` 條）。

## 單一來源

- **帳密登入的步驟推進**只寫在 `run_password_login`：從暫停的那一步重送，token 只用於一次請求。

## 不變量

- `pending_password` 只在「需要驗證」時保存；成功、被拒、改用 QR、錯誤、開始新的帳密登入、驗證取消或逾時（`captcha_solve` 回傳 null）、切到 QR（`qr_start`）時都會清掉，裡面的明文密碼不會多留（2026-09-14 審查後補齊）。
- 密碼只有在帳密登入成功時才交給 `credentials` 加密儲存；被拒、改用 QR、錯誤時都不存。
- 三種登入成功後都把 cookie jar 登記進 `session_stores`，OTP／啟動沿用。
- **GamaPass 的收尾不走 `complete_login`**：token 只發給執行登入的那個 webview，要從它的 cookie 撈出來再 `adopt_cookies` 收進 jar（見總表 `gamapass` 條）。QR 那條才是綁在 `pSKey` 上、我們的 client 收得了尾。

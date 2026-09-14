# commands — Tauri 指令與 AppState

歸屬見 [模組歸屬總表](../規範.md) 的 `commands` 條。

（2026-09-14 首次建檔，隨 [#8](https://github.com/xense999/KZ-Login/issues/8)；以下只記登入相關指令，其餘待動到時補。）

## 公開介面（登入）

- `qr_start` / `qr_check`
- `password_login_start(account, password)` / `password_login_resume(captcha)` → `{ status: "approved", token, games } | { status: "captcha" } | { status: "rejected", message } | { status: "use_qr", message }`；網路錯誤走 `Err(String)`。
- `saved_logins() -> { account, password }[]`、`forget_saved_login(account)`、`reorder_saved_logins(accounts)`：記住的帳密（見總表 `credentials` 條）。
- `captcha_solve(palette, region) -> string | null`、`captcha_cancel()`：替暫停中的帳密登入開驗證視窗（site key 與頁面網址從暫停狀態取，前端不經手）。

## 單一來源

- **帳密登入的步驟推進**只寫在 `run_password_login`：從暫停的那一步重送，token 只用於一次請求。

## 不變量

- `pending_password` 只在「需要驗證」時保存；成功、被拒、改用 QR、錯誤，或開始新的帳密登入時都不保留。
- 密碼只有在帳密登入成功時才交給 `credentials` 加密儲存；被拒、改用 QR、錯誤時都不存。
- 兩種登入成功後都把 cookie jar 登記進 `session_stores`，OTP／啟動沿用。

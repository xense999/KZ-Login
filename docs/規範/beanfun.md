# beanfun — beanfun 網站協定

歸屬見 [模組歸屬總表](../規範.md) 的 `beanfun` 條。

（2026-09-14 首次建檔，隨 [#8](https://github.com/xense999/KZ-Login/issues/8) 帳密登入；以下只記登入相關契約，其餘介面待動到時補。）

## 公開介面（登入）

- `open_login_page(client, skey) -> LoginPage`：載入登入頁並呼叫 `InitLogin`；`LoginPage` 對外有 `skey`（收尾 `complete_login` 要用）、`verification_token`、`captcha_site_key`、`url()`。
- QR：`init_qr_login` → `poll_qr` → `finalize_qr`。
- 帳密：`check_account_type(client, page, account, captcha)`、`account_login(client, page, account, password, captcha)`，都回傳 `LoginStep`（`Proceed`／`CaptchaRequired`／`Rejected(msg)`／`UseQr(msg)`）。
- `complete_login(client, cookie_store, skey) -> bfWebToken`：兩種登入共用的收尾。

## 單一來源

- **登入回應怎麼判讀**只寫在 `read_account_type`／`read_account_login`（純函式，有單元測試，測資含 2026-09-14 實測回應）。
- **登入成功後的收尾**（SendLogin → return.aspx → 讀 bfWebToken）只寫在 `complete_login`；QR 與帳密都呼叫它。

## 不變量

- 看不懂的回應（非 JSON、未知 ResultCode）一律回 `Err`，**絕不**判成「密碼錯」或「要驗證」。
- 要求驗證的判定：`ResultData.IsRecaptcha == true` **或**訊息含「機器人」（CheckAccountType 的回應沒有這個旗標，只有訊息）。
- 檢查帳號類型：`ResultCode` 不是 1 就一律當成拒絕（含要驗證）；送出帳密則只認 0／1／2，其他代碼視為錯誤（2026-09-14 審查後修正）。
- `UseQr` 的訊息＝我們自己的原因說明，beanfun 原文是人看得懂的中文時接在後面；`Success`、`AccountLock` 這類狀態字和進階驗證的導向網址都不顯示給使用者。

## 禁止

- 在本模組外解析 beanfun 回應 JSON —— 正面做法：在本模組加判讀函式並補測試。

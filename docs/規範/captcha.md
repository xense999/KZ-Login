# captcha — reCAPTCHA 驗證視窗

歸屬見 [模組歸屬總表](../規範.md) 的 `captcha` 條。

規格書：[#8](https://github.com/xense999/KZ-Login/issues/8)（2026-09-14 建）

## 公開介面

```rust
pub struct Palette { bg, surface, text, border: String, dark: bool }
pub async fn solve(app, page_url: &str, site_key: &str, palette: &Palette) -> Result<Option<String>, String>
```

- `Ok(Some(token))`＝使用者完成驗證；`Ok(None)`＝取消或逾時（呼叫端一律當「放棄」）；`Err`＝視窗開不起來。
- 唯一呼叫者：`commands` 的 `captcha_solve`。

## 單一來源

- **回傳管道的 fragment 字串**（`kz-captcha=`／`kz-captcha-cancel`）只寫在本模組常數，注入腳本透過替換取得，不另寫一份。
- **配色**來自前端 `styles/main.css` 的 token，由前端讀出後傳進來；本模組不保存調色盤。

## 不變量

- token 只認 beanfun 網域，所以視窗一定載入 beanfun 的登入頁（`login.beanfun.com`），注入腳本只在該 host 上作用。
- 視窗**不掛任何 capability**（零 IPC）；結果只靠後端輪詢視窗網址 fragment 取回（beanfun 的 CSP 本來就擋 app IPC）。
- label 每次換號（`captcha-<n>`）：tauri 的 label 簿記要等 `Destroyed` 才清，用固定 label 會撞號。
- 使用固定、可重用的獨立 WebView2 資料夾（app local data 底下的 `captcha-webview`），不會越開越多；它的瀏覽器參數跟主視窗不同，**不可**跟主視窗共用同一個資料夾。
- 遮罩的 z-index 必須低於 reCAPTCHA 圖片題（約 2e9），否則圖片題會被蓋住。
- 視窗大小、位置跟主視窗外框一致，並蓋在主視窗上；驗證期間主視窗無法被拖動。
- 3 分鐘逾時；結束（不論結果）一律由後端 `destroy`。

## 禁止

- 把整個登入流程放進這個 WebView —— 正面做法：帳密由 `beanfun` 模組走 HTTP，本視窗只負責取 token。WebView2 的追蹤防護會讓 reCAPTCHA 元件失效，OTP／GamaPass 等分支也會失控。
- 從 beanfun 頁面裡把它的驗證元件搬出來用 —— 正面做法：用 `grecaptcha.enterprise.render` 自行 render 在我們的容器裡，beanfun 改版不會影響。

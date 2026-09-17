# captcha — reCAPTCHA 驗證視窗

歸屬見 [模組歸屬總表](../規範.md) 的 `captcha` 條。

規格書：[#8](https://github.com/xense999/KZ-Login/issues/8)（2026-09-14 建）

## 公開介面

```rust
pub struct Palette { bg, text: String, dark: bool }
pub async fn solve(app, page_url: &str, site_key: &str, palette: &Palette, region: overlay::Region) -> Result<Option<String>, String>
pub fn cancel(app)
```

- `Ok(Some(token))`＝使用者完成驗證；`Ok(None)`＝取消、逾時或視窗消失（呼叫端一律當「放棄」）；`Err`＝視窗開不起來。
- 呼叫者：`commands` 的 `captcha_solve`、`captcha_cancel`（登入頁底部「取消」）。

## 單一來源

- **fragment 字串**（`kz-captcha=` 回傳 token）只寫在本模組常數，注入腳本透過替換取得，不另寫一份。
- **驗證區的位置**由前端量測登入頁「標題列與底部按鈕列之間」那塊元素後傳入；本模組不寫死任何版面尺寸。`Region` 與貼上去的動作屬於 `overlay`（見總表），這裡只是用它。
- **配色**來自前端 `styles/main.css` 的 token，由前端讀出後傳進來；本模組不保存調色盤。

## 不變量

- token 只認 beanfun 網域，所以視窗一定載入 beanfun 的登入頁（`login.beanfun.com`），注入腳本只在該 host 上作用。
- 視窗**不掛任何 capability**（零 IPC）；結果只靠後端輪詢視窗網址 fragment 取回（beanfun 的 CSP 本來就擋 app IPC）。
- label 每次換號（`captcha-<n>`）：tauri 的 label 簿記要等 `Destroyed` 才清，用固定 label 會撞號。
- 使用固定、可重用的獨立 WebView2 資料夾（app local data 底下的 `captcha-webview`），不會越開越多；它的瀏覽器參數跟主視窗不同，**不可**跟主視窗共用同一個資料夾。
- 遮罩的 z-index 必須低於 reCAPTCHA 圖片題（約 2e9），否則圖片題會被蓋住。
- 視窗平常只蓋住 `region`（2026-09-14 改：原本蓋整個主視窗）；每 80ms 依主視窗目前位置重新定位，所以拖動主視窗時會跟著移動。
- 視窗**永遠不超出 region**（2026-09-14 使用者定）：圖片題的 iframe 被固定在視窗正中並等比縮小到放得進去（上限 1 倍），而不是放大視窗。
- 驗證區內不放自己的標題或取消鈕；取消一律走登入頁底部「取消」→ `cancel`。
- 3 分鐘逾時；結束（不論結果）一律由後端 `destroy`。

## 禁止

- 把整個登入流程放進這個 WebView —— 正面做法：帳密由 `beanfun` 模組走 HTTP，本視窗只負責取 token。WebView2 的追蹤防護會讓 reCAPTCHA 元件失效，OTP／GamaPass 等分支也會失控。
- 從 beanfun 頁面裡把它的驗證元件搬出來用 —— 正面做法：用 `grecaptcha.enterprise.render` 自行 render 在我們的容器裡，beanfun 改版不會影響。

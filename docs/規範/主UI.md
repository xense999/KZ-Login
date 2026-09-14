# 主 UI — 主視窗頁面

歸屬見 [模組歸屬總表](../規範.md) 的 `主 UI` 條。

（2026-09-14 首次建檔，隨 [#8](https://github.com/xense999/KZ-Login/issues/8)；以下只記登入頁相關契約。）

## 公開介面（登入頁）

- 頁面 `login` 下有兩個子頁：`QrPage`、`PasswordPage`。兩者都 emit `cancel` 與 `success(LoginResult)`；`PasswordPage` 另外 emit `busy(boolean)`。
- 標題列：只有在 `login` 頁時，標題才是「切換登入方式」按鈕；其他頁都是純文字（成功頁維持「新增帳號」）。

- 帳密頁的帳號欄：左側圓點（主頁有對應卡片就是綠色，不論 token 是否失效），右側 ▾ 打開記住的帳密清單（每列都有圓點和 ✕ 刪除；**按住圓點拖曳可排序**，放開時存檔）。

## 單一來源

- **模式怎麼決定**只寫在 `App.vue`：新增帳號讀 localStorage `kusei:login_mode`（預設 `qr`）；重新登入依該帳號的 `loginMethod`，帳密模式帶入 `loginAccount`，密碼由 `PasswordPage` 從記住的清單帶入。
- **登入成功怎麼處理**只寫在 `onLoginSuccess`（新增→成功頁；重新登入，或登入的帳號已有卡片→更新那張卡片、釋放舊 jar，不新增重複的卡片）。

## 不變量

- 只有「新增帳號」時切換才寫入 `kusei:login_mode`；重新登入時切換不寫。
- 帳密送出或驗證中，標題切換按鈕停用。
- 驗證視窗的配色由 `PasswordPage` 從 CSS token 讀出後傳給後端。

## 禁止

- 在前端判讀 beanfun 回應 —— 正面做法：後端回傳已判讀好的 `status`。

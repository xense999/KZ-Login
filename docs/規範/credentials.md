# credentials — 記住的帳密

歸屬見 [模組歸屬總表](../規範.md) 的 `credentials` 條。

規格書：[#8](https://github.com/xense999/KZ-Login/issues/8) 第二輪補充（2026-09-14 建）

## 公開介面

```rust
pub enum LoginKind { Beanfun, Gamapass }
pub struct SavedLogin { account: String, password: String, kind: LoginKind, nickname: Option<String> }
pub fn list(app) -> Result<Vec<SavedLogin>, String>              // 使用者排好的順序
pub fn list_of(app, kind) -> Result<Vec<SavedLogin>, String>
pub fn remember(app, account, password, kind) -> Result<(), String>
pub fn reorder(app, order: &[String]) -> Result<(), String>
pub fn forget(app, account, kind) -> Result<(), String>
pub fn name(app, account, kind, nickname) -> Result<(), String>   // 只替已存在的那一筆補上暱稱
```

呼叫者：`commands` 的 `saved_logins`、`saved_gamapass`、`forget_saved_login`、`forget_gamapass`、`reorder_saved_logins`，以及 `run_password_login`／`gamapass_login` 成功時的 `remember`、`gamapass_login` 讀到暱稱時的 `name`。

## 單一來源

- 檔案位置（app local data 底下的 `credentials.dat`）與加解密只寫在本模組。
- 「同一個帳號」的判定＝去掉前後空白後，英文字母不分大小寫（beanfun 帳號只有 ASCII）：Rust 端寫在 `same_account`，前端對應的是總表 `accounts store` 條的 `sameLoginAccount`；前端所有比對（`findByLoginAccount`、`PasswordPage` 找已存帳密）都只呼叫它。兩邊要一起改。

## 不變量

- 整份清單以 DPAPI（CurrentUser 範圍）加密成一個檔案，不寫登錄檔、不另加鹽；檔案搬到別台電腦或別的 Windows 帳號就解不開。
- **兩種登入的帳密同一個檔案但絕不混用**：GamaPass 帳號不是 beanfun 帳號，出現在對方的清單裡只會登入失敗。每次讀寫都帶 `kind`，比對帳號時也要 `kind` 相同才算同一筆。舊檔沒有這個欄位，`#[serde(default)]` 讓它們照舊算 beanfun。
- **只記對方驗過的密碼**：GamaPass 走「選帳號」那條捷徑、或由使用者接手完成的登入，都不呼叫 `remember`（見總表 `gamapass` 條的 `password_checked`）。
- **`nickname` 只是顯示用的**（GamaPass 的清單拿它取代手機號碼），不參與「同一個帳號」的判定；舊檔沒有這個欄位＝還不知道，前端退回顯示帳號。`name` 對不存在的帳號什麼都不做——光有暱稱不算一筆登入。再次 `remember` 同一個帳號不會洗掉它。
- 只有帳密登入**成功**才儲存；同一個帳號只更新密碼、位置不動，不會新增重複的一列；新帳號接在最後（2026-09-14 改：原本是最近登入排最前）。
- 清單順序＝檔案裡的順序＝使用者在下拉選單拖曳排好的順序。`reorder` 沒提到的帳號（例如選單開著時剛存進來的）保留在最後，不會被刪掉。
- 檔案不存在或解不開時當作空清單，不擋登入頁；下一次儲存會直接覆寫。
- 儲存失敗不影響登入結果，只記一行 log。

## 禁止

- 把密碼存到 localStorage 或任何明文位置 —— 正面做法：一律經過本模組。

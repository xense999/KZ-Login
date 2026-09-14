# credentials — 記住的帳密

歸屬見 [模組歸屬總表](../規範.md) 的 `credentials` 條。

規格書：[#8](https://github.com/xense999/KZ-Login/issues/8) 第二輪補充（2026-09-14 建）

## 公開介面

```rust
pub struct SavedLogin { account: String, password: String }
pub fn list(app) -> Result<Vec<SavedLogin>, String>      // 使用者排好的順序
pub fn remember(app, account, password) -> Result<(), String>
pub fn reorder(app, order: &[String]) -> Result<(), String>
pub fn forget(app, account) -> Result<(), String>
```

呼叫者：`commands` 的 `saved_logins`、`forget_saved_login`、`reorder_saved_logins`，以及 `run_password_login` 成功時的 `remember`。

## 單一來源

- 檔案位置（app local data 底下的 `credentials.dat`）與加解密只寫在本模組。
- 「同一個帳號」的判定＝不分大小寫（`same_account`）。前端 `accounts store` 的 `findByLoginAccount` 用同一條規則，兩邊要一起改。

## 不變量

- 整份清單以 DPAPI（CurrentUser 範圍）加密成一個檔案，不寫登錄檔、不另加鹽；檔案搬到別台電腦或別的 Windows 帳號就解不開。
- 只有帳密登入**成功**才儲存；同一個帳號只更新密碼、位置不動，不會新增重複的一列；新帳號接在最後（2026-09-14 改：原本是最近登入排最前）。
- 清單順序＝檔案裡的順序＝使用者在下拉選單拖曳排好的順序。`reorder` 沒提到的帳號（例如選單開著時剛存進來的）保留在最後，不會被刪掉。
- 檔案不存在或解不開時當作空清單，不擋登入頁；下一次儲存會直接覆寫。
- 儲存失敗不影響登入結果，只記一行 log。

## 禁止

- 把密碼存到 localStorage 或任何明文位置 —— 正面做法：一律經過本模組。

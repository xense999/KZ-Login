# hidden — 隱藏功能的密鑰驗證

歸屬見 [模組歸屬總表](../規範.md) 的 `hidden` 條。

★ 本檔案會隨 GitHub Pages 公開，所以**只寫機制，不寫密鑰、也不寫解鎖入口在哪**。

## 公開介面

```rust
pub fn verify(input: &str) -> Option<&'static str>
```

輸入一組密鑰，回傳它解鎖的功能 id；對不上就是 `None`。沒有其他公開項目。

`commands` 的 `verify_hidden_key` 是唯一呼叫端，前端只拿得到功能 id 字串。

## 單一來源

- **密鑰 → 功能 id 的對照表**只存在 `hidden.rs` 的 `KEYS`。
- **密鑰明文**不在 repo 內的任何檔案：本機放專案根 `.env`（已 gitignore），CI 放同名的 repository secret。
- **雜湊**由 `build.rs` 在編譯期算出並以 `cargo:rustc-env` 注入，程式碼裡只有 `env!()`。
- `HIDDEN_SALT` 在 `build.rs` 與 `hidden.rs` 各有一份，**必須一致**（兩邊都標了註解）。它不是祕密，
  只是讓雜湊對不上公開彩虹表。

## 不變量

- 密鑰環境變數缺席時 `build.rs` **panic，build 直接失敗**。刻意如此：發出一版「密鑰永遠對不上」的
  安裝檔，比 CI 紅燈難發現得多。
- 驗證在 Rust 端，不在前端。前端 bundle 是純文字，雜湊與判斷式一 grep 就找得到。

## 加一個隱藏功能

1. `build.rs` 多一行 `inject_hidden_hash("KZ_HIDDEN_KEY_<NAME>", "KZ_HIDDEN_HASH_<NAME>")`。
2. `hidden.rs` 的 `KEYS` 多一列 `(env!("KZ_HIDDEN_HASH_<NAME>"), "<feature id>")`。
3. `.env`、`.env.example`、CI secret、`release.yml` 的 env 各補一筆。
4. 前端 `useHidden.ts` 的 `FEATURE_NAMES` 補上顯示名稱。

## 禁止

- 把密鑰明文寫進任何進 git 的檔案（含註解、測試、文件）。
- 把雜湊或對照表複製到前端。
- 拿解鎖狀態當安全邊界：它存在 localStorage，會開 DevTools 的人自己塞一筆就開了。
  這道門擋的是誤觸與隨手亂試，不是有心人。

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

- **密鑰 → 功能 id 的對照表**只存在 `hidden.rs` 的 `KEYS`，存的是**雜湊**不是明文。
- **明文密鑰不寫在 repo 內的任何檔案**——包含測試與文件。這個 repo 是公開的，
  明文一旦 commit 就永遠留在 git 歷史裡，事後刪除也下不了架。

## 不變量

- 驗證在 Rust 端，不在前端。前端 bundle 是純文字，雜湊與判斷式一 grep 就找得到。
- **這不是安全邊界**：密鑰很短，雜湊被挖出來後暴力試得出來；解鎖狀態又存在 localStorage，
  會開 DevTools 的人自己塞一筆就開了。這道門擋的是誤觸與隨手亂試。

## 加一個隱藏功能

1. 算出 `SHA-256(HIDDEN_SALT + 明文密鑰)` 的 hex。
2. `KEYS` 多一列 `(那串 hex, "<feature id>")`。
3. 前端 `useHidden.ts` 的 `FEATURE_NAMES` 補上顯示名稱。

## 禁止

- 把明文密鑰寫進任何進 git 的檔案（含註解、測試、文件、commit message）。
- 把雜湊或對照表複製到前端。

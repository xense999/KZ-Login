# release-notes

一個 tag 一個檔：`v<版本>.md`，內容就是 GitHub release 頁面的內文
（`release.yml` 用 `body_path` 直接讀）。

**打 tag 之前要先把對應的 `.md` 寫好並 commit** ——
CI 是從 tag 那個 commit 取檔案的，事後補檔不會回頭改到已發的 release
（那種情況要用 `gh release edit <tag> --notes-file <檔案>` 補）。
檔案不存在時 CI 不會失敗，會退回 `_default.md` 並留一則 warning。

寫法照使用者的更新說明固定格式：整段包在 ```` ```R ```` 圍籬裡，
標題用 `- ` 開頭且寫**改動所在位置**（頁籤名／邏輯設定／獨立視窗名），
內文用 `| ` 開頭，要強調用半形雙引號——圍籬內的 markdown 不會生效，
所以 `**粗體**` 寫了也只會照字面顯示。

內容只寫**這個 tag 相對上一個 tag** 的改動，別把上一版發過的再列一次。

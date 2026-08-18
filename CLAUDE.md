# 久世登入器 (KZ-Login)

Tauri 2 + Vue 3 的 beanfun 登入工具：QR 登入、OTP 取號、一鍵啟動遊戲（走 GGM）、帳號內建瀏覽器（分頁式、雙視窗貼合架構）。

## 文件地圖

- [docs/規範.md](docs/規範.md) — **模組歸屬總表**。每個模組的 owner 程式與契約文件索引；動工前先在這裡落位。
- `docs/規範/<模組名>.md` — 各模組契約（公開介面／單一來源／不變量／禁止）。引用契約一律指向總表條目，不直連模組檔。
- [docs/ggm-data-decrypt.md](docs/ggm-data-decrypt.md) — GGM launch blob 的解密演算法研究紀錄（目前流程刻意不解密，見總表例外區）。
- [docs/瀏覽器重寫目標.md](docs/瀏覽器重寫目標.md) — 帳號內建瀏覽器第一版移除後的目標整理、死因診斷與重寫路線候選。

## Agent skills

### Issue tracker

Issues 與規格書都在 GitHub Issues（xense999/KZ-Login），用 `gh` CLI 操作。See `docs/agents/issue-tracker.md`.

### Domain docs

本 repo 沒有 `docs/術語表.md` / `docs/架構藍圖.md`——模組契約走上面的歸屬總表。`domain-modeling` 若要建那兩份檔，先確認不是與總表平行的第二套結構。

# icon — 應用程式圖標

歸屬見 [模組歸屬總表](../規範.md) 的 `icon` 與 `圖標素材` 兩條。

規格書：[#7](https://github.com/xense999/KZ-Login/issues/7)

## 公開介面

★ 本模組內部有一個私有子模組 `icon::win`（`windows` crate 包 COM）。它與 `lib.rs` 的 `crate::win`（`windows_sys` 包鍵鼠模擬與視窗量測）**同名但毫不相干**——不同 crate、不同職責、無重疊行為。不要合併它們。

```rust
pub enum IconTheme { Light, Dark }
pub fn apply(app: &AppHandle, theme: IconTheme)
```

`apply` 沒有回傳值，**永不失敗**。呼叫端（`commands` 的 `apply_icon_theme`）不需要、也不應該處理錯誤。

## 單一來源

- **主題 → .ico 的映射**只寫在 `IconTheme::icon_file()`。前端只送主題字串，不決定用哪張圖；加第三種顏色時前端不用改。
- **前端主題字彙**（`neutral` / `dark`）的翻譯只發生在 `commands` 的指令邊界。本模組不認得這兩個字串。
- **捷徑位置**只寫在 `shortcut_targets()`。

## 不變量

- **只在啟動時套用一次**。切主題的當下不改任何東西，下次開啟才生效。這個時機同時解掉兩件事：NSIS 更新安裝會重建捷徑、洗掉自訂圖示——下次啟動自動重套；也不必處理「切主題當下捷徑檔正被 Explorer 佔用」的競態。
- **失敗一律靜默**：不中斷、不跳 toast、不影響登入功能，只留一行到 log。
- **log 每次啟動覆寫**（`%LOCALAPPDATA%\久世登入器\icon.log`），天然有界，不需要截斷邏輯。release build 沒有 console，所以 log 必須寫檔才存在。
- **捷徑只改圖示欄位**，不重建捷徑、不動 target——使用者自己加的啟動參數或工作目錄要留著。
- **找不到捷徑就跳過，不補建**。使用者刪掉的捷徑，程式沒有義務把它變回來。

## 禁止

- **不可硬拼 `%USERPROFILE%\Desktop`**。桌面路徑一律走 `SHGetKnownFolderPath(FOLDERID_Desktop)`——桌面被 OneDrive 重導向的使用者硬拼會全數落空。
- **不可為此新增任何設定項**。圖標跟著現有主題走，設定頁不動。
- **不可主動刷新圖示快取**（`SHChangeNotify`），介面也不對釘選工作列的延遲做任何說明。這是刻意的取捨。
- **不可嘗試改寫 exe 內嵌圖示**。那是編譯期資源，執行中的 exe 檔被系統鎖住；「關閉時替換 exe」方案已評估並否決（牽動自我更新流程、可能被防毒誤判）。

## 前提：currentUser 安裝

整個設計成立的前提是 `installer.nsi` 的 `INSTALLMODE = currentUser`——程式裝在 `%LOCALAPPDATA%`，捷徑建在使用者自己的設定檔底下，所以改捷徑**不需要管理員權限**。若哪天改成 perMachine 安裝，這個模組會全面失效（要 UAC），必須重新設計。

## 圖標素材

| 檔案 | 用途 |
|---|---|
| `icons/bunny_cream_1024.png`、`icons/bunny_navy_1024.png` | **來源圖**。所有衍生檔都從這兩張產生 |
| `icons/icon.ico` 與 `icons/*.png`、`icons/icon.icns` | 編譯期內嵌的預設圖示（= cream），由 `tauri icon` 產生 |
| `icons/themed/cream.ico`、`icons/themed/navy.ico` | 執行期用，透過 `tauri.conf.json` 的 `bundle.resources` 打包 |

設計：macOS 風格 squircle（超橢圓 n=5），952/1024 本體、垂直漸層、內緣頂部高光、雙層投影。
兩色：cream `#FFF3DE`、navy `#2E3D59`。

### 加一種新顏色

1. 產一張 1024×1024 的 `icons/bunny_<色名>_1024.png`（來源美術素材不在 repo 內，見下）
2. `npx tauri icon src-tauri/icons/bunny_<色名>_1024.png -o <暫存目錄>`，把產出的 `icon.ico` 複製成 `icons/themed/<色名>.ico`
3. `tauri.conf.json` 的 `bundle.resources` 加一行
4. `IconTheme` 加一個 variant 與對應檔名

`tauri icon` 產出的 .ico 含 16/24/32/48/64/256，沒有 128；缺的尺寸由 Windows 從最接近的那張縮。
規格書 [#7](https://github.com/xense999/KZ-Login/issues/7) 原本寫要含 128，改掉是為了整條產線只用 repo 既有的 `npx tauri icon`——
多一個尺寸不值得為同一個產物多養一套 Python 工具鏈。**沒有實測過 128 缺席在哪些檢視模式下看得出來**，
若日後有人回報某個檢視下圖示糊掉，這裡是第一個要查的地方。

**來源美術素材（那隻兔子的原圖）不在 repo 內。** 這兩張 1024 PNG 就是本 repo 的來源，改設計要從外部素材重新合成。

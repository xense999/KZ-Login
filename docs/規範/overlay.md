# overlay — 貼在主視窗上的子視窗

歸屬見 [模組歸屬總表](../規範.md) 的 `overlay` 條。

（2026-09-17 建檔，隨 [#9](https://github.com/xense999/KZ-Login/issues/9)：`captcha` 與 `gamapass` 都要把一顆無邊框 WebView 貼在主視窗上，原本只寫在 `captcha` 裡。）

## 公開介面

```rust
pub struct Region { x, y, width, height: f64 }   // 主視窗客戶區的 CSS px
pub struct Palette { bg, text: String, dark: bool }   // 前端讀出來的主題色
pub const BROWSER_ARGS: &str
pub fn place(window, main, region)
pub fn disable_tracking_prevention(window)
```

- 使用者：`captcha` 用整組；`gamapass` 只借 `BROWSER_ARGS` 與 `disable_tracking_prevention`——它的視窗是自己的外殼＋內容兩顆，貼法寫在那個模組的 `place_view`。

## 單一來源

- **貼在主視窗某個區域上怎麼對位**（`inner_position` + `scale_factor` 換算）只寫在 `place`。貼在自家外殼裡的是另一回事（`gamapass::place_view`，量的是外殼而不是主視窗）。
- **WebView2 參數**只寫在 `BROWSER_ARGS`：關掉自動化偵測與追蹤防護，兩個視窗要求一致，分開寫遲早會走樣。

## 不變量

- `Region` 是 CSS px、`Palette` 是 CSS token 的值，一律由前端量測／讀出後傳入，Rust 這邊不寫死任何版面尺寸或顏色。（`gamapass` 的視窗尺寸是它自己的常數，不經過這裡。）
- `place` 每個輪詢 tick 都呼叫，不是只在變化時——那正是視窗跟著主視窗移動的方式。

## 禁止

- 在這裡決定「要貼什麼內容」或「什麼時候關掉」 —— 正面做法：那是各流程模組的事，本模組只管貼。

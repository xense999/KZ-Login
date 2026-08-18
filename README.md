# 久世登入器

![Tauri](https://img.shields.io/badge/Tauri-2-blue) ![Vue](https://img.shields.io/badge/Vue-3-42b883) ![Rust](https://img.shields.io/badge/Rust-orange) ![License: MIT](https://img.shields.io/badge/License-MIT-yellow)

遊戲橘子數位科技旗下遊戲的第三方 Beanfun 帳號管理工具。

## 功能

- 同時管理多個 Beanfun 帳號（`新增 / 改名 / 排序 / 刪除`）
- 支持 QR Code 掃碼登入（`透過 Gama Play APP`）
- 連結版本登入（`相機失效時，以登入連結在手機開啟`）
- Discord 通知（`登入連結推送到指定頻道`）
- 帳號改名記憶（`母帳號與子帳號的自訂名稱跨登入自動還原`）
- 智能登入遊戲（`遊戲已開啟就自動填入帳密，未開啟就透過 GGM 啟動`）
- 分享登入 / 代理登入（`複製登入金鑰給他人，對方一鍵代為啟動登入`）
- 帳號內建瀏覽器（`帶著登入態逛官網、會員中心與活動頁，免再登入一次`）
- 亮色 / 暗色主題（`設定後自動記住`）
- 自動檢查遊戲管理員（GGM）更新，可直接下載安裝
- 自動檢查本程式更新，可直接下載安裝

## 系統需求

- Windows 10 / 11

## 下載安裝

從 [GitHub Releases](../../releases) 下載最新版本的 `.exe` 安裝檔。

## 使用方式

:small_orange_diamond:**手動模式**

1. 掃 QR Code 新增帳號
2. 點擊帳號 & 密碼按鈕複製
3. 在遊戲登入框手動貼上

:small_orange_diamond:**自動模式**
1. 點擊帳號旁的「快速登入」按鈕
2. 遊戲已開啟：自動填入帳密並送出登入
3. 遊戲未開啟：自動透過遊戲管理員（GGM）啟動遊戲

:small_orange_diamond:**連結版本登入** `當相機無法正常掃描 QR Code 時使用`

1. 在掃描畫面點擊「連結版本」
2. 登入連結會複製到剪貼簿；若已在設定頁填入 Discord Webhook，會同時推送一個**可點擊的連結**到指定頻道
3. 在手機點開該連結，瀏覽器會自動喚起 APP 完成登入（若未自動跳轉，點頁面上的「手動開啟登入」按鈕即可）

:small_orange_diamond:**帳號內建瀏覽器** `以該帳號的登入身分瀏覽網頁`

1. 點擊帳號頭像開啟（`滑鼠移上去會浮出地球圖示`）
2. 開啟後即為已登入狀態，可直接使用官網、會員中心、活動頁，不需再登入一次
3. 支援多分頁（`Ctrl+T` 開新分頁、`Ctrl+W` 或中鍵點分頁關閉）

**擷圖**

<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/62fcf359-ad25-4272-9672-bc1f8295cd21" />　<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/5a9f30e8-45b3-42e1-bf4b-9746216d7eaf" />
　<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/fec73982-2945-4a8f-957d-e14db5d9fd5c" />　<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/e592dc39-7b94-4728-9079-96a387c6665b" />


## 免責聲明

本軟體**不是**遊戲橘子數位科技股份有限公司所開發的官方客戶端程式。若您的帳號以第三方方式登入，請自行評估風險，並確認下載來源的安全性。

- 本工具不儲存任何帳號密碼；OTP 密碼僅於記憶體中短暫存在，程式關閉後即消失。
- 因使用本工具導致的帳號警告、封鎖或任何損失，作者概不負責。

## 貢獻者

- [Kuze](https://github.com/xense999) — 作者
- [Claude](https://claude.ai) — AI 協作開發

## 致謝

- [pungin/Beanfun](https://github.com/pungin/Beanfun) — Beanfun API 實作參考

## License

MIT

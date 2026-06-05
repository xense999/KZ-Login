# 久世登入器

![Tauri](https://img.shields.io/badge/Tauri-2-blue) ![Vue](https://img.shields.io/badge/Vue-3-42b883) ![Rust](https://img.shields.io/badge/Rust-orange) ![License: MIT](https://img.shields.io/badge/License-MIT-yellow)

遊戲橘子數位科技旗下遊戲的第三方 Beanfun 帳號管理工具。

## 功能

- 同時管理多個 Beanfun 帳號（新增 / 改名 / 排序 / 刪除）
- 支持 QR Code 掃碼登入（透過 Gama Play APP）
- 連結版本登入（相機無法掃描時，改以登入連結在手機開啟）
- Discord Webhook 通知（一鍵將登入連結推送到指定 Discord 頻道）
- 帳號改名記憶（母帳號與子帳號的自訂名稱跨登入自動還原）
- 快速啟動遊戲
- 自動填入帳密

## 系統需求

- Windows 10 / 11

## 下載安裝

從 [GitHub Releases](../../releases) 下載最新版本的 `.msi` 或 `.exe` 安裝檔。

## 使用方式

:small_orange_diamond:**手動模式**

1. 掃 QR Code 新增 Beanfun 帳號
2. 點擊帳號 ID 複製帳號
3. 點擊密碼按鈕複製 OTP
4. 在遊戲登入框手動貼上

:small_orange_diamond:**自動模式**

1. 確認楓之谷登入視窗已開啟
2. 點擊「自動登入」按鈕
3. 應用程式自動填入帳號與 OTP 密碼並送出登入

:small_orange_diamond:**連結版本登入**

> 當相機無法正常掃描 QR Code 時使用：

1. 在掃描畫面點擊「連結版本」
2. 登入連結會複製到剪貼簿；若已在設定頁填入 Discord Webhook，會同時推送到指定頻道
3. 在手機開啟該連結即可觸發 Gama Play APP 登入

**擷圖**

<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/ea952079-5315-4ab2-b8d6-6a5af01353b3" />　<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/e69d7154-0d35-4d67-b26b-fbab63ce1095" />
　<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/fec73982-2945-4a8f-957d-e14db5d9fd5c" />　<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/b8d9f5ac-7fc1-4422-aa23-5ec7d4e09989" />





## 開發環境建置

### 前置需求

- Node.js 18+
- Rust（[rustup](https://rustup.rs)）
- Tauri CLI 2

### 指令

```bash
npm install
npm run tauri dev      # 開發模式
npm run tauri build    # 打包安裝檔
```

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

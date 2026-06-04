# 久世登入器

![Tauri](https://img.shields.io/badge/Tauri-2-blue) ![Vue](https://img.shields.io/badge/Vue-3-42b883) ![Rust](https://img.shields.io/badge/Rust-orange) ![License: MIT](https://img.shields.io/badge/License-MIT-yellow)

遊戲橘子數位科技旗下遊戲的第三方 Beanfun 帳號管理工具。

## 功能

- 同時管理多個 Beanfun 帳號（新增 / 改名 / 排序 / 刪除）
- 支持 QR Code 掃碼登入（透過 Gama Play APP）
- 快速啟動遊戲
- 自動填入帳密

## 系統需求

- Windows 10 / 11

## 下載安裝

從 [GitHub Releases](../../releases) 下載最新版本的 `.msi` 或 `.exe` 安裝檔。

## 使用方式

**手動模式**

1. 掃 QR Code 新增 Beanfun 帳號
2. 點擊帳號 ID 複製帳號
3. 點擊密碼按鈕複製 OTP
4. 在遊戲登入框手動貼上

**自動模式**

1. 確認楓之谷登入視窗已開啟
2. 點擊「自動登入」按鈕
3. 應用程式自動填入帳號與 OTP 密碼並送出登入

**擷圖**

<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/ea952079-5315-4ab2-b8d6-6a5af01353b3" />　　　<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/8eef0927-f69f-4339-8fbe-8576067693d1" />　　<img width="294" height="448" alt="image" src="https://github.com/user-attachments/assets/a3e69754-d863-4a86-9084-f25c7ab99175" />



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

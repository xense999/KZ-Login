# accounts store — 帳號清單

歸屬見 [模組歸屬總表](../規範.md) 的 `accounts store` 條。

（2026-09-14 首次建檔，隨 [#8](https://github.com/xense999/KZ-Login/issues/8)。）

## 公開介面

- 型別：`BeanfunAccount`、`GameAccount`、`LoginMethod`（`"qr" | "password"`）、`LoginResult`（`token`、`games`、`method`、`account`）。
- 動作：`addAccount`、`updateAlias`、`updateGameName`、`removeAccount`、`moveAccount`、`moveGameAccount`、`invalidateToken`、`findByLoginAccount(account)`（不分大小寫，規則與總表 `credentials` 條一致）、`updateToken(accountId, login)`、`markUsed`。

## 單一來源

- `LoginResult` 是兩種登入頁與 App 之間唯一的成功資料形狀。

## 不變量

- 清單只存在記憶體，刻意不持久化（重開程式要重新登入）。
- `loginAccount` 只存帳號，**永不**存密碼（密碼歸總表 `credentials` 條）。
- `updateToken` 遇到 QR 登入（`account` 為 null）時，保留原本的 `loginAccount`。

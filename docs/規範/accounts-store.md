# accounts store — 帳號清單

歸屬見 [模組歸屬總表](../規範.md) 的 `accounts store` 條。

（2026-09-14 首次建檔，隨 [#8](https://github.com/xense999/KZ-Login/issues/8)。）

## 公開介面

- 型別：`BeanfunAccount`、`GameAccount`、`LoginGame`（登入回傳的遊戲帳號 `sn/sid/sname`）、`LoginMethod`（`"qr" | "password"`）、`LoginResult`（`token`、`games`、`method`、`account`）。
- 函式：`sameLoginAccount(a, b)`——前端唯一的「同一個 beanfun 帳號」判定。
- 動作：`addAccount`、`updateAlias`、`updateGameName`、`removeAccount`、`moveAccount`、`moveGameAccount`、`invalidateToken`、`findByGames(games)`（**認卡片的主要依據**：遊戲帳號的 `sn` 是 beanfun 發的序號，不會變、也不會屬於兩個 beanfun 帳號，有一個對得上就是同一張卡片；2026-09-19 使用者定）、`findByLoginAccount(account, method)`（用 `sameLoginAccount`；GamaPass 與 beanfun 是兩個名字空間，`method` 是不是 `gamapass` 要跟卡片的 `loginMethod` 同一邊才算）、`updateToken(accountId, login)`、`markUsed`。

## 單一來源

- `LoginResult` 是兩種登入頁與 App 之間唯一的成功資料形狀。

## 不變量

- 清單只存在記憶體，刻意不持久化（重開程式要重新登入）。
- `loginAccount` 只存帳號，**永不**存密碼（密碼歸總表 `credentials` 條）。
- `updateToken` 遇到沒有帶帳號的登入（QR、使用者接手完成的 GamaPass）時，保留原本的 `loginAccount`——**除非卡片換了邊**（beanfun ↔ GamaPass）：兩邊是不同的名字空間，留著的名字會被拿到錯的那一邊去比對，所以清成 null。

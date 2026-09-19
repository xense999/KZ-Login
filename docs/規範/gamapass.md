# gamapass — 在應用程式內完成的 GamaPass 登入

歸屬見 [模組歸屬總表](../規範.md) 的 `gamapass` 條。

規格書：[#9](https://github.com/xense999/KZ-Login/issues/9)（2026-09-17 建；2026-09-19 改為「看不見的視窗＋記住裝置」，passkey 拔除）

## 公開介面

```rust
pub struct Fill { account: String, password: String, fresh: bool }
pub enum Stage { Working, Code { sent_to, error, attempt }, User }
pub enum Outcome { Completed { token, cookies, password_checked: bool, nickname: Option<String> }, Cancelled }
pub fn ticket() -> u64
pub async fn wait_for_login(app, skey: &str, jar, fill: Fill, region: overlay::Region, ticket: u64, on_stage: impl Fn(&Stage)) -> Result<Outcome, String>
pub fn submit_code(app, code: &str) -> Result<(), String>
pub fn cancel(app)          // 放棄進行中的登入（含還沒開窗的）
pub fn close_windows(app)   // 登入結束後收視窗；不算取消
```

- `fresh`＝新增的帳號：密碼還沒被對方驗過，所以**不走選帳號那條捷徑**，一定經過密碼頁。
- `password_checked`＝腳本真的把密碼送進了對方的密碼頁、而且中途沒有交給使用者。只有這種登入才值得把密碼記下來。
- `ticket` 要在指令的第一步就拿（任何網路請求之前），之後交給 `wait_for_login`。
- `nickname`＝這個帳號在對方那邊的暱稱，腳本讀得到才有；交給使用者接手過的登入一律不帶（登進去的可能是別的帳號）。呼叫端用 `credentials::name` 記下來，清單拿它當顯示名稱。

- 呼叫者：`commands` 的 `gamapass_login`、`gamapass_code`、`gamapass_cancel`（登入頁的「取消」）。
- `Completed` 代表視窗的 cookie 裡出現了 `bfWebToken`，token 與整批 cookie 一起帶回來；呼叫端用 `beanfun::adopt_cookies` 收進自己的 jar。
- `on_stage` 在 `Stage` 每次改變時被叫到；怎麼送到前端（事件名）是 `commands` 的事，本模組不認得。
- `region` 是視窗需要現身時要貼的那一塊（主視窗客戶區的 CSS px），由前端量測後傳入。

## 為什麼這樣做：橘子會記住裝置（2026-09-19 在 Edge 實機確認＋讀對方原始碼）

- **只有「密碼登入」會被記住。** 對方的密碼登入會把「保持登入狀態」的勾選值一起送出（`rememberLoginStatus`）；passkey 登入完全不帶，passkey 替代驗證（驗證碼）那條實測也不會被記住。
- 被記住之後，下次進來落在 `/login/select-account`：列出記住的帳號（遮罩過的手機／信箱），點一下就登入——不問密碼、不問驗證碼、沒有 Windows 安全性的框。最多記 5 組；過期的會退回一般登入。
- 登入態在對方伺服器上，瀏覽器這邊只有 `accounts.gamania.com` 的 cookie（長效，重開瀏覽器仍在）。所以這顆視窗的資料夾就是「這台裝置」。
- **密碼登入成功後對方會推銷 passkey**（「啟用 Passkey (免密碼登入)」／「Passkey 已設定，要設為預設登入嗎?」）。答應了，這個帳號以後按「下一步」就直接走 passkey，再也到不了密碼頁，也就再也不會被記住。
- beanfun 按登出**不會**洗掉對方記住的帳號。

## 畫面

`GamaPassPage` 分兩塊（版面 A，2026-09-19 使用者定）：

- **已記住的帳號**：下拉選單＋「登入」。**顯示的是暱稱，不是手機號碼**（2026-09-19 使用者定）；還不知道暱稱的先顯示帳號。清單來自我們自己的 `credentials`（kind = gamapass）——在這裡用密碼登入成功過的帳號，對方那邊也就記住了。每列可按 ✕ 刪除。沒有記住任何帳號時這一塊不出現。
- **新增帳號**：手機號碼或電子郵件、密碼、「新增並登入」。登入成功才記進清單。
- 沒有 passkey 的入口（2026-09-19 使用者定：直接拔掉）。

登入期間頁面跟著 `gamapass-stage` 事件走：

- `working`：「登入中…」。記住的帳號通常幾秒內就結束，使用者只看到這個。
- `code`：我們自己的驗證碼輸入框（四位數，打滿自動送出）。對方不收時 `error` 帶著對方頁面上的說法再問一次。
- `user`：登入視窗貼到這一頁的內容區上（標題列與底部按鈕列之間），對方的頁面原樣交給使用者。底部的「取消」仍然有效。

## 流程

1. `gamapass_login`：建一個新的 client + cookie jar，`get_session_key` 拿 `pSKey`，`open_login_page` 建立那把 key 的登入頁。
2. 開一顆**停在畫面外**的 WebView 視窗，先停在 `about:blank`，由 `browser::seed_and_navigate` **只清 beanfun 三個網域的舊 cookie**、注入 client 的 cookie，再導向 beanfun 的登入頁。
3. 注入的腳本每 300ms 看一次頁面在哪裡、做那一頁該做的一件事：
   - beanfun 登入頁：按「使用 gamapass」。
   - `/login/select-account`：點遮罩對得上的那一列；對不上或過期就按「使用其他帳號登入」。
   - `/login`：填帳號、勾「保持登入狀態」、按「下一步」。
   - `/login/input-password`：填密碼、按「登入」（只送一次）。
   - 對方跳出「前往驗證」的框（沒見過的裝置要先驗證身分）：替使用者按下去——按了對方才發驗證碼、才進到輸入那一頁。
   - 任何一頁出現驗證碼輸入框（`.input-verification-code`）：回報 `code`，等 `submit_code`。
   - 驗證完成頁（路徑以 `/finished` 結尾）：按「回到 <服務名稱>」——不按，對方不會把人送回 beanfun，token 也就不會發下來。只在這一頁按：同一句話在別處是錯誤框上的「放棄並返回」。
   - passkey 的推銷框：按「繼續使用密碼」／「稍後再說」。**放手之後也照按。**
   - 圖形驗證跳出來、同一頁卡太久、按了三次沒反應的按鈕、沒見過的頁面：回報 `user`，從此不再動手。在選帳號頁放手時，若清單上確實有使用者選的那個帳號（而且沒過期），就把「使用其他帳號登入」與頁尾藏起來，只留清單（2026-09-19 使用者定）；否則那顆按鈕是唯一走得下去的路，留著。
4. 後端每 600ms 讀一次那顆視窗的 cookie（三個 beanfun 網域）與腳本的回報（`browser::eval_json`）：
   - 出現 `bfWebToken` ＝ 登入完成 → 整批 cookie 收進我們的 jar，接著 `get_game_accounts`，登記進 `session_stores`，記住帳密。
   - `Stage` 變了 → `on_stage`；變成 `User` 時把視窗貼到 `region` 上並給它焦點，之後每個 tick 重貼（跟著主視窗移動）。
   - 視窗被 `cancel` 或 10 分鐘沒結果 ＝ 取消。

**為什麼要去 webview 裡撈 cookie**（2026-09-17 實機推翻先前的設計）：QR 登入可以由我們的 client 收尾，因為那條的登入態綁在 `pSKey` 上；**GamaPass 不是**——它把 `bfWebToken` 發給「執行登入的那個瀏覽器」，`complete_login` 在我們的 client 上跑只會得到「任何 cookie 裡都找不到 bfWebToken」。那顆 cookie 同時也是唯一可靠的成功信號：登入成不成功，頁面都會回到 beanfun。

## 單一來源

- **入口網址一律由那個視窗自己去要**（按下 beanfun 登入頁的「使用 gamapass」，由頁面呼叫 `Login/GoGamaPass`）。beanfun 把 OAuth 的 nonce 綁在「提出請求的那條 session」上，我們用 Rust 的 client 代打、再把網址交給視窗，繞回來就是 `AUCB001 參數(nonce)驗證失敗`——即使 cookie 已經複製過去也一樣。寫死 `accounts.gamania.com/login` 更不行：那樣登完會停在橘子那邊，沒有東西回到 portal。
- **登入完成的判定**只寫在本模組的 `harvest`：視窗的 cookie 裡有 `bfWebToken` 才算完成。看網址不算數——失敗也會回到 beanfun。
- **要清哪些 cookie** 只寫在本模組的 `STALE_COOKIE_URLS`；怎麼清、怎麼排在注入之前，屬於 `browser::seed_and_navigate`（見總表 `browser` 條）。
- **視窗貼在哪裡**由前端量測後傳入（`overlay::Region`），貼上去的動作屬於 `overlay`；本模組不寫死任何版面尺寸。

## 不變量

- **只有對方驗過的密碼才記。** 選帳號那條捷徑完全不看密碼——打錯的密碼也登得進去；使用者接手完成的登入，登進去的甚至可能是別的帳號。這兩種都不呼叫 `credentials::remember`（`password_checked = false`）。同理，新增帳號（`fresh`）不准走捷徑：否則任何人隨便打一組密碼就能登進對方還記著的帳號，那組假密碼還會被存起來。
- **舊 token 不算數。** 資料夾是重用的，清 beanfun 的舊 cookie 又是盡力而為的非同步動作；所以開窗當下先記下既有的 `bfWebToken`，`harvest` 只認跟它不同的那一顆。否則清除慢了或失敗了，上一次登入的 token 會被當成這一次成功，而且登進去的是上一個帳號。
- **取消要追得上還沒開窗的登入。** `cancel` 會把計數器加一；登入從第一步就握著當時的值（`ticket`），開窗前、每個 tick 都對一次，對不上就放棄。只靠「關視窗」的話，在拿 session key 那一兩秒按下的取消會落空，接著密碼照打、簡訊照發。
- **收尾只關自己那一顆視窗。** 取消的那條路只 `destroy` 自己的 label；這時候可能已經有下一次登入的視窗開著了。開始新的登入時收掉舊視窗用的是 `close_windows`，不是 `cancel`——後者會讓自己的 `ticket` 當場作廢。
- 讀 cookie 與問腳本都是同步等主執行緒的回呼（頁面正在換的時候會等到逾時），一律丟到 `spawn_blocking`，不佔 runtime 的 worker。

- **`accounts.gamania.com` 的 cookie 永遠不清。** 那是對方記住這台裝置的依據；清了，每次登入都得從頭驗一遍。要清的只有 beanfun 的（上次留下的 token 會讓 portal 短路掉這一次），而且要**刪完才注入、注入完才導向**——各做各的話，晚到的刪除會把剛注入的那批一起帶走。
- **一律走密碼、一律勾「保持登入狀態」、一律婉拒 passkey。** 腳本把 `navigator.credentials.get/create` 換成直接回 `NotAllowedError`（等同使用者在系統的框上按取消）：passkey 登入不會被記住，而且視窗在畫面外，系統的框會憑空冒出來。開了 passkey 優先的帳號會因此走到對方的替代驗證，那一頁交給使用者（`user`），並在畫面上說明原因。
- **視窗是「顯示中但停在畫面外」，不是隱藏。** 隱藏的 WebView 不繪製，對方頁面的轉場不會結束、對話框不會打開。另外帶 `CalculateNativeWinOcclusion` 等參數，免得 Chromium 把畫面外的視窗當成被遮住而降速。★**位置不能交給 builder**：tao 建視窗時，指定的位置不在任何螢幕上就整個丟掉、改用系統預設，無邊框視窗的預設就是螢幕左上角——使用者會看到對方的頁面憑空冒出來（2026-09-19 實機）。所以是「藏著建 → 一次 `SetWindowPos`（移出去＋顯示＋不啟用）」。移跟顯示不能分兩步：tauri 的 `set_position` 只是丟一則訊息給主執行緒，緊接著的顯示可能搶先，頁面就又出現在角落；`show()` 則會把視窗啟用、帶走主視窗的鍵盤焦點。
- **放手之後不收回。** 回報過 `user` 之後，後端不再理會腳本的其他回報，視窗一直留在畫面上到登入結束——頁面已經交給使用者，中途藏起來只會讓他打到一半的東西憑空消失。
- 密碼只在這一次登入的過程中存在：後端只轉交給視窗，成功時才交給 `credentials` 記住（kind = gamapass）。
- **哪張卡片是這次登入的，先看遊戲帳號的 `sn`**（見總表 `主 UI` 條的 `loginTarget`）；指令回傳的 `account` 只是「帳號底下還沒有遊戲帳號」時的退路，而且**只有經過密碼頁的登入才回傳**（`password_checked`）——選帳號那一列是遮罩過的，點進去的「多半」是這個帳號，不到可以替它具名的程度；交給使用者接手過的更不用說。
- 注入的腳本在 `login.beanfun.com` 與 `accounts.gamania.com` 兩個 host 上跑（前者只按那顆「使用 gamapass」），**但帳密只會填進 `accounts.gamania.com`**——別的頁面碰巧載進這個視窗時，一個字都不會被打出去。
- 腳本不偽裝自動化痕跡、不碰任何驗證挑戰；對方要驗就交給使用者。
- 欄位靠 `input` 的 type 找、按鈕靠文字找；用到的少數選擇器是對方原始碼裡手寫的穩定名字（`.use-gama-pass`、`.input-verification-code`、ARIA role），不用框架產生的 class——那種名字改版就會變。
- **暱稱有兩個來源**：選帳號頁那一列上寫著（穩定）；密碼登入之後，從對方頁面自己的狀態（pinia 的 `data` store）裡讀。後者是摸對方的內部結構，摸不到就算了——下次走選帳號那條時會補上，所以新增的帳號最慢第二次登入後才會顯示暱稱。**只在登入完成頁（`/finished`）讀**：在那之前（密碼剛送出、還在等驗證碼）頁面上的個人資料可能還是上一個帳號的，資料夾是重用的。暱稱讀到之後，腳本會等後端讀走（`__kzHeard`）才按會讓頁面離開的按鈕，最多等 2.5 秒。
- 選帳號時，遮罩對得上的列**恰好一列**才點；兩列都對得上就不猜，改走密碼。★已知的殘餘風險：遮罩只露出國碼＋前三碼＋後兩碼，如果使用者有兩個號碼這幾碼都一樣，而要登的那個剛好被對方擠出清單（上限 5 組）或過期，剩下那一列會被誤認。對方的清單不給完整號碼，這邊沒有別的依據可比；發生的條件很窄，先記在這裡。
- 視窗**不掛任何 capability**（零 IPC），同 `captcha`：載入的是外部網站，給它 IPC 等於把 app 的指令開放給那個頁面。腳本的回報由後端主動去問（`browser::eval_json`），驗證碼由後端 `eval` 進去。
- 使用固定、可重用的獨立 WebView2 資料夾（app local data 底下的 `gamapass-webview`），不可與主視窗或 captcha 視窗共用。**刪掉這個資料夾＝對方忘記這台裝置。**
- label 每次換號（`gamapass-<n>`）：tauri 的 label 簿記要等 `Destroyed` 才清，用固定 label 會撞號。
- 10 分鐘逾時；成功的那條路由呼叫端關視窗（收尾失敗時那個畫面是唯一的線索）。

## 禁止

- 自己實作 GamaPass 的登入 API —— 正面做法：登入請求要帶對方頁面才產得出的 reCAPTCHA v3 token，只能在他們的頁面上完成；我們負責的是把輸入接過來。
- 提供 passkey 登入 —— 正面做法：passkey 每次都要過 Windows 驗證、而且不會被對方記住，與這個模組要達成的事相反。只有 passkey 的帳號請使用者到 gamapass 會員中心設定密碼並關閉「優先使用 Passkey」。
- 拿 `complete_login` 來收尾這條登入 —— 正面做法：token 在那個視窗的 cookie 裡，撈出來收進 jar。`complete_login` 是 QR 那條的收尾，在這裡只會失敗（而且它會 POST `return.aspx`，不是唯讀）。
- 登入前 `DeleteAllCookies` —— 正面做法：只清 `STALE_COOKIE_URLS`（v2.4.0 就是整個清空，所以每次都得重新驗證）。

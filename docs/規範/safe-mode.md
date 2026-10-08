# safe-mode — 安全模式（鎖定主視窗的六位數密碼）

歸屬見 [模組歸屬總表](../規範.md) 的 `safe_mode` 條。

## 公開介面

```rust
pub fn status(app) -> Result<Status, String>          // { has_pin, auto_lock, lock_on_start }
pub fn set_pin(app, pin, current: Option<&str>) -> Result<(), String>
pub fn lock(app) -> Result<(), String>
pub fn verify(app, pin) -> Result<bool, String>
pub fn set_auto_lock(app, on) -> Result<(), String>
pub fn clear(app) -> Result<(), String>
```

對應的指令：`safe_mode_status`、`safe_mode_set_pin(pin, current)`、`safe_mode_lock()`、`safe_mode_verify(pin)`、
`safe_mode_set_auto_lock(on)`、`safe_mode_reset()`。

前端只透過 `src/composables/useSafeMode.ts` 呼叫這些指令；鎖定畫面是
`src/components/LockScreen.vue`，設定密碼的視窗是 `src/components/SafePinDialog.vue`，
兩者共用輸入框 `src/components/PinField.vue`。設定頁的「安全模式」卡片只有一列入口（設好密碼後多一顆
鎖頭），所有選項都在 `src/components/SafeSettingsDialog.vue` 這個設定視窗裡。

## 單一來源

- **密碼與「開啟時自動進入」**只存在 `safe_mode.dat`（`app_local_data_dir`），整份是一個 DPAPI blob
  （見總表 `dpapi` 條）。只有 `safe_mode.rs` 讀寫這個檔。
- **這個視窗現在鎖著沒有**只存在前端 `useSafeMode` 的 `locked`。
- **手動鎖了還沒解**記在同一個檔裡（`locked`）：`lock` 寫下、`verify` 對了才清掉。開場要不要鎖
  ＝`status` 的 `lock_on_start`（開了自動進入，或手動鎖了還沒解），所以關掉重開、再開一個實例都繞不過去。
- **右鍵快捷安全模式**（主頁空白處按右鍵要不要出現「進入安全模式」；帳號卡片上的右鍵選單不放）只存在前端 `useSafeMode` 的 `quickLock`
  （localStorage `kusei:safe_quick_lock`）。它只決定選單露不露出來，不是鎖定條件；沒有密碼時一律不露，
  重設時一起關掉。
- **密碼格式**（六位 ASCII 數字）由 `safe_mode.rs` 的 `valid_pin` 判定；前端的 `PIN_LENGTH` 只管輸入框長度。

## 不變量

- 密碼明文不離開後端：前端只能交一組數字進來問對不對，拿不到已存的密碼。
- 已經有密碼時，變更密碼必須帶對目前的密碼。
- 沒有密碼就不能開「開啟時自動進入」，`status` 在沒有密碼時一律回報 `auto_lock: false`。
- 檔案不存在或解不開（別的 Windows 帳號複製來的、損毀）都當成「沒有密碼」——擋在外面會讓擁有者永遠進不來。
  其他讀檔錯誤（暫時讀不到）回報錯誤，不當成沒有密碼。存檔是寫到旁邊再換上去，別的實例不會讀到寫一半的。
- `safe_mode_reset`（忘記密碼）依序清掉：GamaPass 那邊對這台裝置的記憶
  （`gamapass::forget_device`，不清的話重設後仍可免密碼登入；會因資料夾被佔用而失敗，所以排第一，
  失敗時什麼都還沒動）→ 記住的帳密 → 帳號瀏覽器視窗 → 所有登入狀態 → 安全模式密碼。
  密碼最後清，中途任何一步失敗就不解鎖。
- 重設只清得到執行它的那個實例：同時開著的其他實例，畫面上的帳號與登入狀態不受影響（已知、未處理）。
- 前端在解鎖**之前**清掉畫面上的帳號與各頁記住的帳號資訊，並由 `App.vue` 看 `wasReset` 把頁面帶回主頁
  ——鎖定畫面一解鎖就被卸載，它自己發的事件送不出去。
- 鎖定期間已登入的帳號不登出，背景保活照常跑；底下的頁面設為 `inert`，鍵盤走不進去。
- 輸錯不限次數（使用者拍板）。

## 這不是什麼

- **這不是加密邊界**：擋的是坐到電腦前隨手操作的人。記住的帳密仍是原本的 DPAPI 保護，
  沒有另外綁安全模式密碼；能以同一個 Windows 帳號刪掉 `safe_mode.dat` 的人就能略過鎖定。
- 帳號瀏覽器視窗不在鎖定範圍內：鎖定前已開著的會留著（重設時才會被關掉）。

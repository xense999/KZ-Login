import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

// 安全模式：主視窗鎖住，輸入六位數密碼才能繼續用。密碼只有後端知道（加密存檔），
// 這裡拿得到的只有「有沒有設」「開啟時要不要鎖」，以及問後端某組數字對不對。
// 現在鎖著沒有＝這裡的 locked；null 是開場還沒問到後端，那段時間一律當成鎖著。
export const PIN_LENGTH = 6;

type Status = { has_pin: boolean; auto_lock: boolean };

const hasPin = ref(false);
const autoLock = ref(false);
const locked = ref<boolean | null>(null);
// 每重設一次加一：資料被清空了，殼層要把頁面帶回主頁
const wasReset = ref(0);

async function refresh() {
  const s = await invoke<Status>("safe_mode_status");
  hasPin.value = s.has_pin;
  autoLock.value = s.auto_lock;
}

export function useSafeMode() {
  // 開場呼叫一次。問不到後端就不鎖：鎖了也沒有密碼可以對，等於把人關在外面。
  async function initSafeMode() {
    try {
      await refresh();
      locked.value = hasPin.value && autoLock.value;
    } catch {
      locked.value = false;
    }
  }

  function lock() {
    if (hasPin.value) locked.value = true;
  }

  async function unlock(pin: string) {
    const ok = await invoke<boolean>("safe_mode_verify", { pin });
    if (ok) locked.value = false;
    return ok;
  }

  async function setPin(pin: string, current: string | null) {
    await invoke("safe_mode_set_pin", { pin, current });
    await refresh();
  }

  async function setAutoLock(on: boolean) {
    await invoke("safe_mode_set_auto_lock", { on });
    autoLock.value = on;
  }

  // 忘記密碼的出路：後端把密碼、記住的帳密、登入狀態全清掉。畫面上的帳號由呼叫端
  // 在 clearLocal 裡清——要趕在解鎖之前，解鎖那一刻底下的頁面就露出來了。
  async function resetSafeMode(clearLocal: () => void) {
    await invoke("safe_mode_reset");
    clearLocal();
    wasReset.value += 1;
    hasPin.value = false;
    autoLock.value = false;
    locked.value = false;
  }

  return { hasPin, autoLock, locked, wasReset, initSafeMode, lock, unlock, setPin, setAutoLock, resetSafeMode };
}

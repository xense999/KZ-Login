<script setup lang="ts">
import { ref, onMounted, nextTick } from "vue";
import PinField from "./PinField.vue";
import { useSafeMode } from "../composables/useSafeMode";
import { useAccountsStore } from "../stores/accounts";

// 安全模式的鎖定畫面：蓋住標題列以下的整個程式，密碼對了才拿開。
// 忘記密碼的出路是清掉全部資料——留著資料放行就等於沒鎖。
const emit = defineEmits<{ reset: [] }>();

const { unlock, resetSafeMode } = useSafeMode();
const store = useAccountsStore();

// 各頁記在這台電腦上的帳號資訊（別名、子帳號名稱與順序、上次用的 GamaPass 帳號）。
// 重設時一起清：留著等於把帳號名單留給按下重設的人。
const ACCOUNT_MEMORY_KEYS = ["kusei:alias_memory", "kusei:name_memory", "kusei:suborder_memory", "kusei:gamapass_last"];

const pin = ref("");
const error = ref("");
const busy = ref(false);
const forgetting = ref(false);
const field = ref<InstanceType<typeof PinField> | null>(null);

onMounted(() => field.value?.focus());

async function submit() {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    if (!(await unlock(pin.value))) {
      error.value = "密碼錯誤";
      pin.value = "";
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
    // 停用期間輸入框會掉焦點，放開後接回來才能直接再打
    nextTick(() => field.value?.focus());
  }
}

async function cancelForget() {
  forgetting.value = false;
  error.value = "";
  await nextTick();
  field.value?.focus();
}

async function reset() {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await resetSafeMode();
    store.clear();
    for (const key of ACCOUNT_MEMORY_KEYS) localStorage.removeItem(key);
    emit("reset");
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="lock">
    <template v-if="!forgetting">
      <svg class="icon" viewBox="0 0 24 24" fill="none" width="34" height="34" aria-hidden="true">
        <rect x="5" y="10.5" width="14" height="9.5" rx="2.5" stroke="currentColor" stroke-width="1.6"/>
        <path d="M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      </svg>
      <div class="hd">
        <h2>安全模式</h2>
        <p>輸入六位數密碼以繼續使用</p>
      </div>
      <div class="form">
        <PinField ref="field" v-model="pin" :disabled="busy" @complete="submit" @input="error = ''" />
        <div class="err">{{ error }}</div>
      </div>
      <button class="link" @click="forgetting = true; error = ''">忘記密碼？</button>
    </template>

    <template v-else>
      <div class="hd">
        <h2>忘記密碼</h2>
        <p>
          重設會清除安全模式密碼，並一併清除所有帳號、已記住的帳號密碼與登入狀態，登入器回到全新狀態。<br />
          這個動作無法復原。
        </p>
      </div>
      <div class="form">
        <div class="err">{{ error }}</div>
        <div class="actions">
          <button class="btn" :disabled="busy" @click="cancelForget">取消</button>
          <button class="btn danger" :disabled="busy" @click="reset">清除並重設</button>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.lock {
  /* 蓋在 .page-container 上，比應用內對話框（2100）高：鎖住時底下什麼都不該露出來 */
  position: absolute;
  inset: 0;
  z-index: 3000;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 18px;
  padding: 22px 18px;
  background: var(--bg);
}
.icon { color: var(--text3); }

.hd { text-align: center; max-width: 300px; }
.hd h2 { font-size: 16px; font-weight: 600; color: var(--text); letter-spacing: -0.01em; }
.hd p { font-size: 13px; line-height: 1.6; color: var(--text2); margin-top: 6px; }

.form { width: 100%; max-width: 220px; }
.err {
  min-height: 20px;
  margin-top: 8px;
  font-size: 12px;
  text-align: center;
  color: var(--red);
}

.link {
  border: none;
  background: none;
  font-size: 12px;
  color: var(--text3);
  transition: color 0.15s;
}
.link:hover { color: var(--text2); }

.actions { display: flex; gap: 8px; margin-top: 6px; }
.btn {
  flex: 1;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
  font-size: 13px;
  font-weight: 500;
  color: var(--text2);
  white-space: nowrap;
  transition: background 0.12s, color 0.12s;
}
.btn:hover:not(:disabled) { background: var(--surface2); color: var(--text); }
.btn:disabled { opacity: 0.5; cursor: default; }
.btn.danger { color: var(--red); }
.btn.danger:hover:not(:disabled) { background: rgba(255, 69, 58, 0.12); color: var(--red); }
</style>

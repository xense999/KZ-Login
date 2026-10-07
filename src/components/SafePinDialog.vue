<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import PinField from "./PinField.vue";
import { useSafeMode, PIN_LENGTH } from "../composables/useSafeMode";
import { toast } from "../composables/useToast";

// 設定／變更安全模式密碼。已經有密碼時要先輸入目前的那組，對不對由後端判斷。
const emit = defineEmits<{ close: [] }>();

const { hasPin, setPin } = useSafeMode();
// 開著的時候密碼會從「沒有」變「有」，欄位不能跟著多一格出來
const changing = hasPin.value;

const current = ref("");
const next = ref("");
const again = ref("");
const error = ref("");
const saving = ref(false);
const first = ref<InstanceType<typeof PinField> | null>(null);

onMounted(() => first.value?.focus());

const ready = computed(() =>
  next.value.length === PIN_LENGTH &&
  again.value.length === PIN_LENGTH &&
  (!changing || current.value.length === PIN_LENGTH));

async function submit() {
  if (!ready.value || saving.value) return;
  if (next.value !== again.value) {
    error.value = "兩次輸入的密碼不一樣";
    return;
  }
  saving.value = true;
  error.value = "";
  try {
    await setPin(next.value, changing ? current.value : null);
    toast(changing ? "已變更安全模式密碼" : "已設定安全模式密碼");
    emit("close");
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Teleport to=".page-container">
  <div class="overlay" @click.self="emit('close')" @keydown.escape="emit('close')">
    <div class="card">
      <div class="title">{{ changing ? "變更安全模式密碼" : "設定安全模式密碼" }}</div>
      <div class="fields" @input="error = ''" @keydown.enter="submit">
        <PinField v-if="changing" ref="first" v-model="current" placeholder="目前的密碼" />
        <PinField v-if="changing" v-model="next" placeholder="新密碼（六位數字）" />
        <PinField v-else ref="first" v-model="next" placeholder="密碼（六位數字）" />
        <PinField v-model="again" placeholder="再輸入一次" />
      </div>
      <div v-if="error" class="err">{{ error }}</div>
      <div class="actions">
        <button class="btn" @click="emit('close')">取消</button>
        <button class="btn primary" :disabled="!ready || saving" @click="submit">確定</button>
      </div>
    </div>
  </div>
  </Teleport>
</template>

<style scoped>
.overlay {
  /* absolute 不是 fixed：Teleport 到 .page-container，所以蓋的是標題列以下的
     整個程式範圍，圓角由視窗外框裁切。 */
  position: absolute;
  inset: 0;
  z-index: 2100;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(0, 0, 0, 0.42);
  backdrop-filter: blur(3px);
  -webkit-backdrop-filter: blur(3px);
}
.card {
  width: 100%;
  max-width: 300px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 20px;
  box-shadow: var(--ctx-shadow);
}
.title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
  margin-bottom: 14px;
}
.fields { display: flex; flex-direction: column; gap: 8px; }
.err {
  margin-top: 8px;
  font-size: 12px;
  color: var(--red);
}
.actions {
  display: flex;
  gap: 8px;
  margin-top: 18px;
}
.btn {
  flex: 1;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface2);
  color: var(--text2);
  font-size: 13px;
  cursor: pointer;
}
.btn:hover { background: var(--surface3); color: var(--text); }
.btn:disabled { opacity: 0.5; cursor: default; }
.btn.primary {
  background: var(--primary-bg);
  border-color: var(--primary-border);
  color: var(--primary-color);
}
.btn.primary:hover:not(:disabled) { background: var(--primary-bg-hover); }
</style>

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useHidden, FEATURE_NAMES } from "../composables/useHidden";
import { toast } from "../composables/useToast";

const emit = defineEmits<{ close: [] }>();

const { toggle } = useHidden();
const value = ref("");
const error = ref("");
const checking = ref(false);
const input = ref<HTMLInputElement | null>(null);

onMounted(() => input.value?.focus());

async function submit() {
  const key = value.value.trim();
  if (!key || checking.value) return;
  checking.value = true;
  error.value = "";
  try {
    const feature = await invoke<string | null>("verify_hidden_key", { key });
    if (!feature) {
      error.value = "密鑰錯誤";
      return;
    }
    const on = toggle(feature);
    const name = FEATURE_NAMES[feature] ?? feature;
    toast(on ? `已開啟隱藏功能：${name}` : `已關閉隱藏功能：${name}`);
    emit("close");
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    checking.value = false;
  }
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="card">
      <div class="title">隱藏功能密鑰</div>
      <input
        ref="input"
        v-model="value"
        class="field"
        type="password"
        autocomplete="off"
        spellcheck="false"
        @keydown.enter="submit"
        @keydown.escape="emit('close')"
        @input="error = ''"
      />
      <div v-if="error" class="err">{{ error }}</div>
      <div class="actions">
        <button class="btn" @click="emit('close')">取消</button>
        <button class="btn primary" :disabled="!value.trim() || checking" @click="submit">確定</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
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
.field {
  width: 100%;
  padding: 9px 11px;
  border: 1px solid var(--input-border);
  border-radius: 9px;
  background: var(--input-bg);
  color: var(--text);
  font-size: 13px;
  outline: none;
}
.field:focus { border-color: var(--primary-border); }
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

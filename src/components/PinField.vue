<script setup lang="ts">
import { ref } from "vue";
import { PIN_LENGTH } from "../composables/useSafeMode";

// 六位數密碼的輸入框：只收數字、遮起來、滿六位時說一聲。鎖定畫面與設定密碼的視窗共用。
const model = defineModel<string>({ required: true });
defineProps<{ placeholder?: string; disabled?: boolean }>();
const emit = defineEmits<{ complete: [] }>();

const el = ref<HTMLInputElement | null>(null);

function onInput(e: Event) {
  const input = e.target as HTMLInputElement;
  const digits = input.value.replace(/\D/g, "").slice(0, PIN_LENGTH);
  // 被濾掉的字要從畫面上拿走：model 沒變的話 Vue 不會重畫這一格
  input.value = digits;
  model.value = digits;
  if (digits.length === PIN_LENGTH) emit("complete");
}

defineExpose({ focus: () => el.value?.focus() });
</script>

<template>
  <input
    ref="el"
    class="pin"
    type="password"
    inputmode="numeric"
    autocomplete="off"
    :maxlength="PIN_LENGTH"
    :value="model"
    :placeholder="placeholder"
    :disabled="disabled"
    @input="onInput"
  />
</template>

<style scoped>
.pin {
  width: 100%;
  padding: 10px 11px;
  border: 1px solid var(--input-border);
  border-radius: 10px;
  background: var(--input-bg);
  color: var(--text);
  font-size: 16px;
  letter-spacing: 0.4em;
  text-align: center;
  outline: none;
  transition: border-color 0.15s;
}
.pin::placeholder { font-size: 13px; letter-spacing: 0; color: var(--text3); }
.pin:focus { border-color: var(--primary-border); }
.pin:disabled { opacity: 0.6; }
</style>

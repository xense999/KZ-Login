<script setup lang="ts">
import { useToast } from "../composables/useToast";

const { message, kind, visible } = useToast();
</script>

<template>
  <Transition name="toast">
    <div v-if="visible" class="toast-pop" :class="kind">{{ message }}</div>
  </Transition>
</template>

<style scoped>
/* 浮動提示樣式的單一來源；觸發用 composables/useToast 的 toast()。
   置於標題列下方置中，反色 pill；error 走紅色變體。 */
.toast-pop {
  position: fixed;
  top: 52px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 1000;
  max-width: calc(100% - 32px);
  padding: 10px 16px;
  border-radius: 12px;
  background: var(--text);
  color: var(--bg);
  font-size: 13px;
  line-height: 1.4;
  text-align: center;
  box-shadow: 0 6px 24px rgba(0, 0, 0, 0.25);
}
.toast-pop.error {
  background: var(--red);
  color: #fff;
}
.toast-enter-active,
.toast-leave-active { transition: opacity 0.2s, transform 0.2s; }
.toast-enter-from,
.toast-leave-to { opacity: 0; transform: translateX(-50%) translateY(-8px); }
</style>

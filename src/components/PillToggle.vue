<script setup lang="ts" generic="T extends string | number | boolean">
import { computed } from "vue";

// 滑動切換，照久世管理器的主題切換（外形是圓角矩形，不是膠囊——使用者要的）：所有選項都顯示，選中的那個被一塊浮起來的膠囊
// 托著，切換時滑過去、帶一點回彈。底槽跟膠囊開關、下拉選單共用同一道凹槽。
// 每個選項一樣寬（滑塊靠這個算位置），以最長的那個為準。
const props = defineProps<{
  modelValue: T;
  options: { value: T; label: string; tip?: string }[];
}>();
const emit = defineEmits<{ "update:modelValue": [value: T] }>();

const index = computed(() => Math.max(0, props.options.findIndex((o) => o.value === props.modelValue)));
</script>

<template>
  <div class="pilltoggle" :style="{ '--n': options.length, '--i': index }">
    <i class="knob"></i>
    <button
      v-for="o in options"
      :key="String(o.value)"
      :class="{ on: o.value === modelValue }"
      :data-tip="o.tip"
      @click="emit('update:modelValue', o.value)"
    >
      {{ o.label }}
    </button>
  </div>
</template>

<style scoped>
.pilltoggle {
  position: relative;
  flex: none;
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: 1fr;
  height: 34px;
  padding: 2px;
  border-radius: 8px;
  background: var(--trough);
  box-shadow: var(--trough-ring);
}
.knob {
  position: absolute;
  top: 2px;
  bottom: 2px;
  left: 2px;
  width: calc((100% - 4px) / var(--n));
  border-radius: 6px;
  background: var(--seg-on);
  box-shadow: var(--switch-knob-shadow);
  transform: translateX(calc(var(--i) * 100%));
  transition: transform 0.38s cubic-bezier(0.34, 1.4, 0.64, 1), scale 0.18s ease;
}
/* 按住時膠囊微微鼓起來 */
.pilltoggle:active .knob {
  scale: 1.06 1.1;
}
button {
  position: relative;
  height: 100%;
  padding: 0 18px;
  /* 中文字型的上下留白不對稱，行高不設 1 的話字會被推得偏一點 */
  line-height: 1;
  border: none;
  border-radius: 6px;
  background: transparent;
  font-size: 13px;
  /* 沒選中跟選中同一個字色、只差粗細，同管理器 */
  font-weight: 500;
  color: var(--text);
  white-space: nowrap;
  transition: font-weight 0.2s ease;
}
button.on {
  font-weight: 700;
}
</style>

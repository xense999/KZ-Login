<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref } from "vue";

/**
 * 主視窗的滑鼠提示，取代原生 title（顯示慢、樣式不受控）。
 *
 * 單例＋position:fixed＋事件委派：任何元素加 data-tip 就會生效，不必改這支；
 * 也不會被祖先的 overflow:hidden 或捲動容器裁掉。
 *
 * ★只給主視窗用。帳號瀏覽器的工具列視窗只有一條高，畫在視窗裡的提示放不下，
 * 那邊維持原生 title（原生的可以超出視窗）。
 */
const GAP = 8;
const EDGE = 8;

const text = ref("");
const x = ref(0);
const y = ref(0);
const box = ref<HTMLElement | null>(null);

function place(target: HTMLElement) {
  const r = target.getBoundingClientRect();
  const b = box.value?.getBoundingClientRect();
  if (!b) return;

  // 貼在上緣、水平置中；超出左右就往內推，不讓它被切掉
  const half = b.width / 2;
  const center = r.left + r.width / 2;
  x.value = Math.min(Math.max(center, EDGE + half), window.innerWidth - EDGE - half);

  // 上面放不下就翻到下面
  const above = r.top - GAP - b.height;
  y.value = above >= EDGE ? above : r.bottom + GAP;
}

function onOver(e: MouseEvent) {
  const el = (e.target as HTMLElement | null)?.closest?.("[data-tip]") as HTMLElement | null;
  const tip = el?.dataset.tip;
  if (!el || !tip) {
    text.value = "";
    return;
  }
  text.value = tip;
  // 先畫出來才量得到寬高，量完才知道要不要閃邊
  void nextTick(() => place(el));
}

function onOut(e: MouseEvent) {
  const from = (e.target as HTMLElement | null)?.closest?.("[data-tip]");
  const to = (e.relatedTarget as HTMLElement | null)?.closest?.("[data-tip]");
  if (from && from !== to) text.value = "";
}

// 點下去之後目標常常就換了樣子或消失（展開、換頁），提示留著只會擋東西
function hide() {
  text.value = "";
}

onMounted(() => {
  document.addEventListener("mouseover", onOver);
  document.addEventListener("mouseout", onOut);
  document.addEventListener("mousedown", hide, true);
  document.addEventListener("scroll", hide, true);
});
onUnmounted(() => {
  document.removeEventListener("mouseover", onOver);
  document.removeEventListener("mouseout", onOut);
  document.removeEventListener("mousedown", hide, true);
  document.removeEventListener("scroll", hide, true);
});
</script>

<template>
  <div
    v-show="text"
    ref="box"
    class="tip"
    :style="{ left: `${x}px`, top: `${y}px` }"
    aria-hidden="true"
  >
    {{ text }}
  </div>
</template>

<style scoped>
/* 反色，跟 ToastPop 同一組配色。視窗只有 420 寬而有些說明很長，所以限寬、可換行；
   圓角 14＝單行時剛好是膠囊，多行時是圓角方塊 */
.tip {
  position: fixed;
  z-index: 1100;
  transform: translateX(-50%);
  width: max-content;
  max-width: calc(100% - 16px);
  padding: 7px 13px;
  font-size: 12px;
  font-weight: 500;
  line-height: 1.5;
  white-space: pre-line;
  color: var(--bg);
  background: var(--text);
  border-radius: 14px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.22);
  pointer-events: none;
}
</style>

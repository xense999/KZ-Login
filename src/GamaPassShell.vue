<script setup lang="ts">
/**
 * GamaPass 登入視窗的外殼：畫邊框與標題列，網頁本身是疊在框裡的另一個視窗
 * （同帳號瀏覽器的架構——外部網站的頁面裡畫不了我們自己的標題列）。
 * 這裡沒有分頁也沒有網址列：只有一頁，而且不給亂逛。
 */
import { Window } from "@tauri-apps/api/window";

// 跟 Rust 端的版面常數對齊，改了要一起改（見 gamapass 模組）。
const TITLEBAR_H = 42;
const EDGE = 3;

function close() {
  Window.getCurrent().close();
}
</script>

<template>
  <div class="frame" :style="{ '--titlebar-h': `${TITLEBAR_H}px`, '--frame-edge': `${EDGE}px` }">
    <div class="titlebar shell-titlebar" data-tauri-drag-region>
      <span class="title" data-tauri-drag-region>GamaPass 登入</span>
      <span class="titlebar-space" data-tauri-drag-region></span>
      <button class="wbtn close" title="關閉" @click="close">&#x2715;</button>
    </div>
  </div>
</template>

<style scoped>
/* 邊框用 inset box-shadow 畫、不用 border：border 會把子元素的定位原點推進來
   一圈，貼在框裡的那顆視窗就會對不齊。 */
.frame {
  position: relative;
  width: 100%;
  height: 100%;
  background: var(--bg);
  box-shadow: inset 0 0 0 var(--frame-edge) var(--edge);
  border-radius: 10px 10px 0 0;
  overflow: hidden;
}

.shell-titlebar {
  box-sizing: border-box;
  height: var(--titlebar-h);
  padding: 0 8px 0 14px;
  gap: 6px;
}

.title {
  flex-shrink: 0;
  font-size: 13px;
  font-weight: 500;
  color: var(--text2);
  letter-spacing: 0.01em;
}

.titlebar-space { flex: 1; height: 100%; }
</style>

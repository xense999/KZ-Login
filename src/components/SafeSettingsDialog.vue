<script setup lang="ts">
import { ref } from "vue";
import SafePinDialog from "./SafePinDialog.vue";
import { useSafeMode } from "../composables/useSafeMode";
import { toast } from "../composables/useToast";

// 安全模式的設定視窗。版型照久世管理器的設定視窗：標題列＋右上 X、一類一張卡片
// （卡片標題左邊一條主色直線）、最下面一條線分開的按鈕列。改了就存，沒有儲存鈕。
const emit = defineEmits<{ close: [] }>();

const { hasPin, autoLock, quickLock, lock, setAutoLock, setQuickLock } = useSafeMode();
const showPin = ref(false);

// 鎖上之後把這個視窗收掉：解鎖回來不該還停在設定視窗裡
async function enterSafeMode() {
  try {
    await lock();
    emit("close");
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), { kind: "error" });
  }
}

const NEED_PIN = "請先設定安全模式密碼";

async function toggleAutoLock() {
  try {
    await setAutoLock(!autoLock.value);
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), { kind: "error" });
  }
}
</script>

<template>
  <Teleport to=".page-container">
    <div class="overlay" @click.self="emit('close')" @keydown.escape="emit('close')">
      <div class="win">
        <div class="head">
          <span>安全模式設定</span>
          <button class="close" data-tip="關閉（設定改了就存）" @click="emit('close')">
            <svg viewBox="0 0 12 12" width="11" height="11" aria-hidden="true">
              <path d="M3 3 9 9M9 3 3 9" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
            </svg>
          </button>
        </div>
        <div class="body">
          <section class="ccard">
            <div class="chead">{{ hasPin ? "安全模式" : "密碼" }}</div>
            <div v-if="hasPin" class="setrow">
              <span class="setkey">進入安全模式</span>
              <button class="setbtn icon" @click="enterSafeMode"
                data-tip="立刻進入安全模式，輸入密碼才能繼續使用。已登入的帳號不會被登出">
                <svg viewBox="0 0 24 24" fill="none" width="14" height="14" aria-hidden="true">
                  <rect x="5" y="10.5" width="14" height="9.5" rx="2.5" stroke="currentColor" stroke-width="1.8"/>
                  <path d="M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
                </svg>
              </button>
            </div>
            <div v-else class="setrow">
              <span class="setkey">安全模式密碼</span>
              <button class="setbtn" data-tip="密碼是六位數字，加密後存在這台電腦上"
                @click="showPin = true">設定</button>
            </div>
          </section>
          <section class="ccard">
            <div class="chead">進入方式</div>
            <div class="setrow">
              <span class="setkey">開啟時自動進入安全模式</span>
              <button
                class="pill-switch"
                role="switch"
                :class="{ on: autoLock }"
                :aria-checked="autoLock"
                :disabled="!hasPin"
                :data-tip="hasPin ? '每次開啟登入器都先鎖住，輸入密碼才能使用' : NEED_PIN"
                @click="toggleAutoLock"
              >
                <span class="pill-knob"></span>
              </button>
            </div>
            <div class="setrow">
              <span class="setkey">右鍵快捷安全模式</span>
              <button
                class="pill-switch"
                role="switch"
                :class="{ on: quickLock && hasPin }"
                :aria-checked="quickLock && hasPin"
                :disabled="!hasPin"
                :data-tip="hasPin ? '開啟後，在主頁面的空白處按右鍵會出現「進入安全模式」' : NEED_PIN"
                @click="setQuickLock(!quickLock)"
              >
                <span class="pill-knob"></span>
              </button>
            </div>
          </section>
          <div class="foot">
            <button class="setbtn primary" @click="emit('close')">完成</button>
          </div>
        </div>
      </div>
    </div>
    <SafePinDialog v-if="showPin" @close="showPin = false" />
  </Teleport>
</template>

<style scoped>
.overlay {
  /* absolute 不是 fixed：Teleport 到 .page-container，蓋的是標題列以下的整個程式範圍 */
  position: absolute;
  inset: 0;
  z-index: 2050;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  background: rgba(0, 0, 0, 0.42);
  backdrop-filter: blur(2px);
  -webkit-backdrop-filter: blur(2px);
}
.win {
  width: 100%;
  max-height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--float);
  border: 1px solid var(--border);
  border-radius: 14px;
  box-shadow: var(--ctx-shadow);
  overflow: hidden;
}
.head {
  flex: none;
  display: flex;
  align-items: center;
  height: 46px;
  padding: 0 10px 0 16px;
  font-size: 16px;
  font-weight: 700;
  color: var(--text);
  border-bottom: 1px solid var(--border2);
}
.close {
  margin-left: auto;
  width: 26px;
  height: 26px;
  padding: 0;
  border: none;
  background: none;
  color: var(--text3);
  border-radius: 6px;
  transition: background 0.12s, color 0.12s;
}
.close:hover { background: var(--glass-hover); color: var(--text); }
.body {
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
}
.body > * { flex-shrink: 0; }

.ccard {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 0 12px 10px;
  background: var(--surface2);
  border: 1px solid var(--border);
  border-radius: 10px;
}
.chead {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 38px;
  margin: 0 -12px 4px;
  padding: 0 12px;
  font-size: 14px;
  font-weight: 700;
  color: var(--text);
  border-bottom: 1px solid var(--border2);
}
.chead::before {
  content: "";
  flex: none;
  width: 4px;
  height: 1.2em;
  border-radius: 2px;
  background: var(--primary-color);
}
.setrow {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 32px;
  font-size: 14px;
  color: var(--text);
  white-space: nowrap;
}
.setbtn.icon { width: 28px; padding: 0; }
.setbtn {
  flex: none;
  height: 28px;
  /* 下面多 1px：同設定頁的小按鈕，中文字量過偏下 0.6px */
  padding: 0 14px 1px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 7px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text2);
  transition: background 0.12s, color 0.12s;
}
.setbtn:hover { background: var(--surface3); color: var(--text); }
.setbtn.primary {
  background: var(--primary-bg);
  border-color: var(--primary-border);
  color: var(--primary-color);
}
.setbtn.primary:hover { background: var(--primary-bg-hover); }
.foot {
  display: flex;
  justify-content: flex-end;
  padding-top: 12px;
  border-top: 1px solid var(--border2);
}
.foot .setbtn { height: 32px; padding: 0 22px 1px; font-size: 13px; }

/* 開關跟設定頁同一顆（外觀與 34×19 的大小都照那邊） */
.pill-switch {
  flex: none;
  width: 34px;
  height: 19px;
  padding: 2px;
  border: none;
  border-radius: 999px;
  background: color-mix(in srgb, var(--text) 7%, transparent);
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--text) 8%, transparent),
    inset 0 1px 1px color-mix(in srgb, var(--text) 6%, transparent);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: flex-start;
  transition: background 0.18s ease;
}
.pill-switch:disabled { cursor: default; opacity: 0.45; }
.pill-switch.on { background: var(--primary-color); }
.pill-knob {
  width: 15px;
  height: 15px;
  border-radius: 50%;
  background: #fff;
  box-shadow: var(--switch-knob-shadow);
  transition: transform 0.18s ease;
}
.pill-switch.on .pill-knob { transform: translateX(15px); }
</style>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

const props = defineProps<{ sid: string; otp: string; fetchedAt: number }>();

const secondsLeft = ref(30);
const copiedSid = ref(false);
const copiedOtp = ref(false);
let timer: ReturnType<typeof setInterval> | null = null;

const fetchTime = computed(() => {
  const d = new Date(props.fetchedAt);
  const h = d.getHours().toString().padStart(2, "0");
  const m = d.getMinutes().toString().padStart(2, "0");
  const s = d.getSeconds().toString().padStart(2, "0");
  return `${h}:${m}:${s}`;
});

const isExpired = computed(() => secondsLeft.value <= 0);

const progressPct = computed(() =>
  Math.max(0, Math.min(100, (secondsLeft.value / 30) * 100))
);

onMounted(() => {
  const elapsed = Math.floor((Date.now() - props.fetchedAt) / 1000);
  secondsLeft.value = Math.max(0, 30 - elapsed);
  if (secondsLeft.value > 0) {
    timer = setInterval(() => {
      secondsLeft.value--;
      if (secondsLeft.value <= 0 && timer) { clearInterval(timer); timer = null; }
    }, 1000);
  }
});

onUnmounted(() => { if (timer) clearInterval(timer); });

async function copySid() {
  await writeText(props.sid);
  copiedSid.value = true;
  setTimeout(() => (copiedSid.value = false), 1800);
}

async function copyOtp() {
  await writeText(props.otp);
  copiedOtp.value = true;
  setTimeout(() => (copiedOtp.value = false), 1800);
}
</script>

<template>
  <div class="otp-card">
    <!-- 帳號 -->
    <div class="cred-row">
      <span class="cred-lbl">帳號</span>
      <span class="cred-val" :title="sid">{{ sid }}</span>
      <button class="btn-copy" :class="{ done: copiedSid }" @click="copySid">
        {{ copiedSid ? "已複製" : "複製" }}
      </button>
    </div>

    <div class="divider"></div>

    <!-- 密碼 -->
    <div class="cred-row">
      <span class="cred-lbl">密碼</span>
      <span class="cred-val otp" :class="{ expired: isExpired }">{{ otp }}</span>
      <div class="otp-right">
        <span class="timer-badge" :class="{ expired: isExpired }">
          {{ isExpired ? "已過期" : `${secondsLeft}s` }}
        </span>
        <button class="btn-copy" :class="{ done: copiedOtp }" :disabled="isExpired" @click="copyOtp">
          {{ copiedOtp ? "已複製" : "複製" }}
        </button>
      </div>
    </div>

    <!-- 進度條 -->
    <div class="progress-track" v-if="!isExpired">
      <div
        class="progress-bar"
        :style="{ width: progressPct + '%' }"
        :class="{ warning: secondsLeft <= 10 }"
      ></div>
    </div>

    <div class="meta-row">取得於 {{ fetchTime }}</div>
  </div>
</template>

<style scoped>
.otp-card {
  margin: 0 14px 12px 22px;
  background: var(--surface2);
  border-radius: 10px;
  overflow: hidden;
  animation: slideDown 0.2s ease;
}

@keyframes slideDown {
  from { opacity: 0; transform: translateY(-6px); }
  to   { opacity: 1; transform: translateY(0); }
}

.cred-row {
  display: flex;
  align-items: center;
  padding: 10px 12px;
  gap: 8px;
}

.divider {
  height: 1px;
  background: var(--border2);
  margin: 0 12px;
}

.cred-lbl {
  font-size: 11px;
  font-weight: 600;
  color: var(--text3);
  width: 30px;
  flex-shrink: 0;
  letter-spacing: 0.02em;
}

.cred-val {
  flex: 1;
  font-size: 14px;
  font-weight: 500;
  color: var(--text);
  font-variant-numeric: tabular-nums;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cred-val.otp {
  font-family: "SF Mono", "Menlo", "Monaco", monospace;
  font-size: 18px;
  font-weight: 700;
  letter-spacing: 0.25em;
  color: #fff;
}
.cred-val.expired { color: var(--text3); }

.otp-right { display: flex; align-items: center; gap: 7px; flex-shrink: 0; }

.timer-badge {
  font-size: 11px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--amber);
  background: rgba(255, 214, 10, 0.12);
  padding: 2px 7px;
  border-radius: 20px;
}
.timer-badge.expired {
  color: var(--red);
  background: rgba(255, 69, 58, 0.12);
}

.btn-copy {
  background: rgba(255, 255, 255, 0.08);
  border: none;
  border-radius: 6px;
  padding: 4px 10px;
  font-size: 11px;
  font-weight: 500;
  color: var(--text2);
  transition: background 0.12s, color 0.12s;
  flex-shrink: 0;
}
.btn-copy:hover:not(:disabled) { background: rgba(255,255,255,0.14); color: var(--text); }
.btn-copy:disabled { opacity: 0.3; cursor: default; }
.btn-copy.done { background: rgba(48, 209, 88, 0.15); color: var(--green); }

/* Progress */
.progress-track {
  height: 2px;
  background: var(--border2);
  margin: 0;
}
.progress-bar {
  height: 100%;
  background: var(--blue);
  transition: width 1s linear, background 0.3s;
}
.progress-bar.warning { background: var(--warn); }

.meta-row {
  font-size: 10px;
  color: var(--text3);
  padding: 5px 12px 7px;
  letter-spacing: 0.01em;
}
</style>

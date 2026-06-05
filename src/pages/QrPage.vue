<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

const emit = defineEmits<{
  cancel: [];
  success: [token: string, games: { sn: string; sid: string; sname: string }[]];
}>();

type Status = "loading" | "waiting" | "expired" | "error";
const status = ref<Status>("loading");
const qrImage = ref("");
const deeplink = ref("");
const errorMsg = ref("");
const linkCopied = ref(false);
let pollTimer: ReturnType<typeof setTimeout> | null = null;

onMounted(() => startQr());
onUnmounted(() => { if (pollTimer) clearTimeout(pollTimer); });

async function startQr() {
  if (pollTimer) { clearTimeout(pollTimer); pollTimer = null; }
  status.value = "loading"; qrImage.value = ""; deeplink.value = ""; errorMsg.value = "";
  try {
    const result = await invoke<{ bitmap_base64: string; deeplink?: string }>("qr_start");
    qrImage.value = result.bitmap_base64;
    deeplink.value = result.deeplink ?? "";
    status.value = "waiting";
    schedulePoll();
  } catch (e) { status.value = "error"; errorMsg.value = String(e); }
}

async function copyDeeplink() {
  if (!deeplink.value) return;
  await writeText(deeplink.value);
  linkCopied.value = true;
  setTimeout(() => { linkCopied.value = false; }, 2000);
}

function schedulePoll() { pollTimer = setTimeout(poll, 2000); }

async function poll() {
  type R = { status: "waiting" } | { status: "expired" } | { status: "approved"; token: string; games: { sn: string; sid: string; sname: string }[] };
  try {
    const r = await invoke<R>("qr_check");
    if (r.status === "approved") { emit("success", r.token, r.games); return; }
    if (r.status === "expired") { status.value = "expired"; return; }
    status.value = "waiting"; schedulePoll();
  } catch (e) { status.value = "error"; errorMsg.value = String(e); }
}
</script>

<template>
  <div class="qr-page">
    <div class="qr-hd">
      <h2>掃描 QR Code</h2>
      <p>請用手機 Gama Play APP 掃描</p>
    </div>

    <div class="qr-body">
      <template v-if="status === 'loading'">
        <div class="spinner-lg"></div>
        <span class="status-txt">取得 QR Code…</span>
      </template>

      <template v-else-if="status === 'waiting'">
        <div class="qr-frame" @click="startQr" title="點擊刷新">
          <img :src="qrImage" class="qr-img" alt="QR Code" />
          <div class="qr-refresh">↺ 刷新</div>
        </div>
        <div class="pulse-row">
          <div class="pulse-dot"></div>
          <div class="pulse-dot" style="animation-delay:.25s"></div>
          <div class="pulse-dot" style="animation-delay:.5s"></div>
        </div>
        <span class="status-txt">等待掃描</span>
        <button v-if="deeplink" class="btn-link" @click="copyDeeplink">
          {{ linkCopied ? "已複製 ✓" : "連結版本" }}
        </button>
      </template>

      <template v-else-if="status === 'expired'">
        <div class="state-card">
          <div class="state-icon">⏱</div>
          <div class="state-title">QR Code 已過期</div>
        </div>
      </template>

      <template v-else-if="status === 'error'">
        <div class="state-card">
          <div class="state-icon">⚠</div>
          <div class="state-title">發生錯誤</div>
          <div class="state-sub">{{ errorMsg }}</div>
        </div>
      </template>
    </div>

    <div class="actions">
      <button class="btn-ghost" @click="$emit('cancel')">取消</button>
      <button v-if="status === 'expired' || status === 'error'" class="btn-solid" @click="startQr">重新取得</button>
    </div>
  </div>
</template>

<style scoped>
.qr-page {
  display: flex; flex-direction: column; align-items: center;
  gap: 18px; padding: 22px 18px 18px; flex: 1;
}

.qr-hd { text-align: center; }
.qr-hd h2 { font-size: 16px; font-weight: 600; color: var(--text); letter-spacing: -0.01em; }
.qr-hd p  { font-size: 13px; color: var(--text2); margin-top: 4px; }

.qr-body {
  flex: 1; display: flex; flex-direction: column;
  align-items: center; justify-content: center; gap: 14px; width: 100%;
}

/* QR Frame */
.qr-frame {
  position: relative; width: 210px; height: 210px;
  border-radius: 16px; overflow: hidden;
  background: #fff; cursor: pointer;
  box-shadow: 0 8px 32px rgba(0,0,0,0.5);
  flex-shrink: 0;
}
.qr-img { width: 100%; height: 100%; object-fit: contain; display: block; padding: 8px; }
.qr-refresh {
  position: absolute; inset: 0;
  display: flex; align-items: center; justify-content: center;
  font-size: 14px; font-weight: 600; color: #fff;
  background: rgba(0,0,0,0.48);
  opacity: 0; transition: opacity 0.18s;
  backdrop-filter: blur(3px);
}
.qr-frame:hover .qr-refresh { opacity: 1; }

.pulse-row { display: flex; gap: 5px; }
.pulse-dot {
  width: 5px; height: 5px; border-radius: 50%; background: var(--green);
  animation: pulse 1.5s ease infinite;
}
@keyframes pulse { 0%,100%{opacity:1;transform:scale(1);} 50%{opacity:.2;transform:scale(.6);} }

.status-txt { font-size: 13px; color: var(--text2); }

.btn-link {
  background: none;
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 6px 16px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text3);
  transition: background 0.12s, color 0.12s;
}
.btn-link:hover { background: var(--glass-hover); color: var(--text2); }

.spinner-lg {
  width: 32px; height: 32px;
  border: 2px solid rgba(255,255,255,0.07);
  border-top-color: rgba(255,255,255,0.5);
  border-radius: 50%; animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }

.state-card {
  background: var(--surface);
  border: var(--card-border);
  border-radius: 14px; padding: 24px 20px;
  display: flex; flex-direction: column; align-items: center; gap: 8px;
  width: 100%; max-width: 260px;
  box-shadow: var(--card-shadow);
  backdrop-filter: var(--card-filter);
  -webkit-backdrop-filter: var(--card-filter);
}
.state-icon { font-size: 30px; opacity: 0.7; }
.state-title { font-size: 14px; font-weight: 500; color: var(--text); }
.state-sub { font-size: 12px; color: var(--text2); text-align: center; word-break: break-all; }

.actions { display: flex; gap: 7px; width: 100%; }

.btn-ghost {
  flex: 1; background: var(--surface); border: 1px solid var(--border);
  border-radius: 10px; padding: 11px;
  font-size: 13px; font-weight: 500; color: var(--text2);
  transition: background 0.15s, color 0.15s;
}
.btn-ghost:hover { background: var(--surface2); color: var(--text); }

.btn-solid {
  flex: 1; background: var(--primary-bg); border: 1px solid var(--primary-border);
  border-radius: 10px; padding: 11px;
  font-size: 13px; font-weight: 600; color: var(--primary-color);
  transition: background 0.15s;
}
.btn-solid:hover { background: var(--primary-bg-hover); }
</style>

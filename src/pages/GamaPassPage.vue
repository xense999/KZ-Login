<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { LoginGame, LoginResult } from "../stores/accounts";

const emit = defineEmits<{
  cancel: [];
  success: [login: LoginResult];
}>();

type Result =
  | { status: "approved"; token: string; games: LoginGame[] }
  | { status: "cancelled" };

const regionEl = ref<HTMLElement | null>(null);
const waiting = ref(false);
const errorMsg = ref("");
// Switching away unmounts this page while the window may still be up; its
// reply must not act on a page nobody is looking at any more.
let disposed = false;

onMounted(() => {
  // Landing here is the request — the page is the login page, so there is
  // nothing to press first.
  start();
});

onUnmounted(() => {
  disposed = true;
  invoke("gamapass_cancel").catch(() => { /* 視窗早就關了 */ });
});

// Where the login window sits: the strip between the title bar and the bottom
// bar, in CSS pixels of the main window.
function readRegion() {
  const r = regionEl.value!.getBoundingClientRect();
  return { x: r.left, y: r.top, width: r.width, height: r.height };
}

async function start() {
  errorMsg.value = "";
  waiting.value = true;
  // The strip has to be laid out before it can be measured.
  await nextTick();
  try {
    const result = await invoke<Result>("gamapass_login", { region: readRegion() });
    if (disposed) return;
    if (result.status === "approved") {
      emit("success", {
        token: result.token,
        games: result.games,
        method: "gamapass",
        // The account is typed into Gamania's page, not ours — same as QR.
        account: null,
      });
      return;
    }
    // Cancelled: the window is gone, so leave the page rather than sit on an
    // empty strip with nothing in it.
    emit("cancel");
  } catch (e: unknown) {
    if (!disposed) errorMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (!disposed) waiting.value = false;
  }
}

// While the login window is up it is what the user is looking at, so cancel
// closes it; `start` then leaves the page on its own.
function onCancel() {
  if (waiting.value) invoke("gamapass_cancel").catch(() => { /* 已經關了 */ });
  else emit("cancel");
}
</script>

<template>
  <div class="gp-page">
    <div ref="regionEl" class="gp-main">
      <template v-if="errorMsg">
        <div class="state">
          <div class="state-icon err">⚠</div>
          <div class="state-title">登入沒有完成</div>
          <div class="state-sub">{{ errorMsg }}</div>
        </div>
      </template>
      <template v-else-if="waiting">
        <!-- 登入頁蓋在這塊上面，這裡只是它還沒畫出來時的底 -->
        <div class="spinner-lg"></div>
      </template>
    </div>

    <div class="bottom-bar">
      <button class="btn-ghost" @click="onCancel">取消</button>
      <button v-if="errorMsg" class="btn-solid" @click="start">重新登入</button>
    </div>
  </div>
</template>

<style scoped>
.gp-page { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.gp-main {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  gap: 12px; padding: 22px 18px 18px; flex: 1; min-height: 0;
}

.state { display: flex; flex-direction: column; align-items: center; gap: 6px; text-align: center; }
.state-icon { font-size: 24px; }
.state-icon.err { color: var(--red); }
.state-title { font-size: 14px; font-weight: 600; color: var(--text); }
.state-sub { font-size: 12px; color: var(--text2); max-width: 260px; line-height: 1.6; }

/* 同掃碼頁的等待指示器（scoped 樣式各自為政，共用的只有 main.css 的 token）。 */
.spinner-lg {
  width: 32px; height: 32px;
  border: 2px solid rgba(255,255,255,0.07);
  border-top-color: rgba(255,255,255,0.5);
  border-radius: 50%; animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }
</style>

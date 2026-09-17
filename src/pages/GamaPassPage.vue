<script setup lang="ts">
import { ref, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { LoginGame, LoginResult } from "../stores/accounts";

const emit = defineEmits<{
  cancel: [];
  success: [login: LoginResult];
  busy: [value: boolean];
}>();

type Result =
  | { status: "approved"; token: string; games: LoginGame[] }
  | { status: "cancelled" };

const waiting = ref(false);
const errorMsg = ref("");
// Switching away unmounts this page while the window may still be open; the
// reply must not act on a page nobody is looking at any more.
let disposed = false;

onUnmounted(() => {
  disposed = true;
  if (waiting.value) invoke("gamapass_cancel").catch(() => { /* 視窗早就關了 */ });
});

async function start() {
  waiting.value = true;
  errorMsg.value = "";
  emit("busy", true);
  try {
    const result = await invoke<Result>("gamapass_login");
    if (disposed) return;
    if (result.status === "approved") {
      emit("success", {
        token: result.token,
        games: result.games,
        method: "gamapass",
        // The account is typed into Gamania's page, not ours — same as QR.
        account: null,
      });
    }
  } catch (e: unknown) {
    if (!disposed) errorMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (!disposed) {
      waiting.value = false;
      emit("busy", false);
    }
  }
}

function cancelWindow() {
  invoke("gamapass_cancel").catch(() => { /* 視窗早就關了 */ });
}
</script>

<template>
  <div class="gp-page">
    <div class="gp-main">
      <div class="gp-hd">
        <h2>GamaPass 登入</h2>
        <p>在遊戲橘子的登入頁完成</p>
      </div>

      <div class="gp-body">
        <template v-if="waiting">
          <div class="spinner-lg"></div>
          <span class="status-txt">請在另一個視窗完成登入…</span>
          <span class="hint">登入成功後這裡會自動接手。關掉那個視窗就取消。</span>
        </template>

        <template v-else-if="errorMsg">
          <div class="state">
            <div class="state-icon err">⚠</div>
            <div class="state-title">發生錯誤</div>
            <div class="state-sub">{{ errorMsg }}</div>
          </div>
        </template>

        <template v-else>
          <div class="note">
            密碼與驗證都在遊戲橘子自己的頁面上完成，登入器不會經手，也不會記住。
          </div>
        </template>
      </div>
    </div>

    <div class="bottom-bar">
      <button class="btn-ghost" @click="waiting ? cancelWindow() : $emit('cancel')">取消</button>
      <button v-if="!waiting" class="btn-solid" @click="start">
        {{ errorMsg ? "重新登入" : "開啟登入頁" }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.gp-page { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.gp-main {
  display: flex; flex-direction: column; align-items: center;
  gap: 18px; padding: 22px 18px 18px; flex: 1; min-height: 0;
}

.gp-hd { text-align: center; }
.gp-hd h2 { font-size: 16px; font-weight: 600; color: var(--text); letter-spacing: -0.01em; }
.gp-hd p  { font-size: 13px; color: var(--text2); margin-top: 4px; }

.gp-body {
  flex: 1; display: flex; flex-direction: column;
  align-items: center; justify-content: center; gap: 12px; width: 100%;
}

.note {
  font-size: 13px; line-height: 1.7; color: var(--text2);
  text-align: center; max-width: 280px;
}
.hint { font-size: 12px; color: var(--text3); text-align: center; max-width: 260px; line-height: 1.6; }
.status-txt { font-size: 13px; color: var(--text2); }

/* 同掃碼頁的等待指示器（scoped 樣式各自為政，共用的只有 main.css 的 token）。 */
.spinner-lg {
  width: 32px; height: 32px;
  border: 2px solid rgba(255,255,255,0.07);
  border-top-color: rgba(255,255,255,0.5);
  border-radius: 50%; animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }

.state { display: flex; flex-direction: column; align-items: center; gap: 6px; text-align: center; }
.state-icon { font-size: 24px; }
.state-icon.err { color: var(--red); }
.state-title { font-size: 14px; font-weight: 600; color: var(--text); }
.state-sub { font-size: 12px; color: var(--text2); max-width: 260px; line-height: 1.6; }
</style>

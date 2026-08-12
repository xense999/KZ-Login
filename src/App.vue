<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { Window } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import MainPage from "./pages/MainPage.vue";
import QrPage from "./pages/QrPage.vue";
import SuccessPage from "./pages/SuccessPage.vue";
import SettingsPage from "./pages/SettingsPage.vue";
import { useAccountsStore } from "./stores/accounts";

type Page = "main" | "qr" | "success" | "settings";

const page = ref<Page>("main");
const pendingToken = ref<string | null>(null);
const pendingGames = ref<{ sn: string; sid: string; sname: string }[]>([]);
const reauthAccountId = ref<string | null>(null);
const store = useAccountsStore();

const pageTitles: Record<Page, string> = {
  main: "久世登入器",
  qr: "新增帳號",
  success: "新增帳號",
  settings: "設定",
};

function minimize() { Window.getCurrent().minimize(); }
function close() { Window.getCurrent().close(); }

let keepAliveTimer: ReturnType<typeof setInterval> | null = null;

async function checkSessions() {
  for (const acc of store.accounts) {
    if (!acc.token) continue;
    try {
      const alive = await invoke<boolean>('ping_session', { token: acc.token });
      if (!alive) store.invalidateToken(acc.id);
    } catch { /* 忽略，下次再試 */ }
  }
}

onMounted(() => {
  checkSessions();
  keepAliveTimer = setInterval(checkSessions, 3 * 60 * 1000);
});

onUnmounted(() => {
  if (keepAliveTimer) clearInterval(keepAliveTimer);
});

function onReauth(accountId: string) {
  reauthAccountId.value = accountId;
  page.value = "qr";
}

async function onQrSuccess(token: string, games: { sn: string; sid: string; sname: string }[]) {
  if (reauthAccountId.value) {
    await store.updateToken(reauthAccountId.value, token, games);
    reauthAccountId.value = null;
    page.value = "main";
    return;
  }
  pendingToken.value = token;
  pendingGames.value = games;
  page.value = "success";
}

function onAccountSaved() {
  pendingToken.value = null;
  pendingGames.value = [];
  page.value = "main";
}
</script>

<template>
  <div class="app-window">
    <div class="titlebar" data-tauri-drag-region>
      <button class="wbtn settings-btn" @click="page = page === 'settings' ? 'main' : 'settings'" :class="{ active: page === 'settings' }">
        <svg viewBox="0 0 16 16" fill="none" width="14" height="14">
          <circle cx="8" cy="8" r="2.5" stroke="currentColor" stroke-width="1.4"/>
          <path d="M8 1v2M8 13v2M1 8h2M13 8h2M3.05 3.05l1.41 1.41M11.54 11.54l1.41 1.41M3.05 12.95l1.41-1.41M11.54 4.46l1.41-1.41"
            stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
        </svg>
      </button>
      <span class="title" data-tauri-drag-region>{{ pageTitles[page] }}</span>
      <div class="win-controls">
        <button class="wbtn" @click="minimize">&#x2212;</button>
        <button class="wbtn close" @click="close">&#x2715;</button>
      </div>
    </div>

    <div class="page-container">
      <MainPage v-if="page === 'main'" @add-account="page = 'qr'" @reauth="onReauth" />
      <QrPage v-else-if="page === 'qr'" @cancel="page = 'main'" @success="onQrSuccess" />
      <SuccessPage v-else-if="page === 'success'" :token="pendingToken!" :games="pendingGames" @saved="onAccountSaved" />
      <SettingsPage v-else-if="page === 'settings'" @back="page = 'main'" />
    </div>
  </div>
</template>

<style scoped>
.app-window {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: var(--bg);
  border-radius: 14px;
  overflow: hidden;
  box-shadow: none;
}

.titlebar {
  height: 42px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  padding: 0 14px;
  cursor: default;
  background: var(--titlebar-bg);
  backdrop-filter: var(--titlebar-blur);
  -webkit-backdrop-filter: var(--titlebar-blur);
  border-bottom: 1px solid var(--border2);
}

.title {
  font-size: 13px;
  font-weight: 500;
  color: var(--text2);
  letter-spacing: 0.01em;
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  pointer-events: none;
}

.settings-btn {
  margin-right: auto;
  color: var(--text3);
}
.settings-btn:hover { background: var(--glass-hover); color: var(--text2); }
.settings-btn.active { color: var(--primary-color); background: var(--interactive-active-bg); }

.win-controls {
  display: flex;
  gap: 2px;
}

.wbtn {
  width: 26px;
  height: 22px;
  background: none;
  border: none;
  color: var(--text3);
  font-size: 12px;
  border-radius: 5px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s, color 0.15s;
}
.wbtn:hover { background: var(--glass-hover); color: var(--text2); }
.wbtn.close:hover { background: rgba(255,69,58,0.15); color: var(--red); }

.page-container {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
</style>

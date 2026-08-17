<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { Window } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import MainPage from "./pages/MainPage.vue";
import QrPage from "./pages/QrPage.vue";
import SuccessPage from "./pages/SuccessPage.vue";
import SettingsPage from "./pages/SettingsPage.vue";
import ToastPop from "./components/ToastPop.vue";
import { toast } from "./composables/useToast";
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

const updateAsk = ref(false);
const updateInfo = ref<{ current: string; server: string; url: string } | null>(null);

async function checkGgmUpdate() {
  try {
    const u = await invoke<{ current: string; server: string; has_update: boolean; url: string }>(
      "check_ggm_update",
    );
    if (!u.has_update || !u.url) return;
    updateInfo.value = { current: u.current, server: u.server, url: u.url };
    updateAsk.value = true;
  } catch (e) {
    // best-effort; never block startup on the update check
    console.error(e);
  }
}

async function confirmUpdate() {
  const info = updateInfo.value;
  updateAsk.value = false;
  if (!info) return;
  try {
    await invoke("update_ggm", { url: info.url });
    toast("已開始下載並開啟安裝程式，請依畫面指示完成更新", { ms: 4000 });
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), { kind: "error" });
  }
}

onMounted(() => {
  checkSessions();
  checkGgmUpdate();
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

    <div v-if="updateAsk" class="modal-overlay" @click.self="updateAsk = false">
      <div class="modal-card">
        <div class="modal-title">遊戲管理員更新</div>
        <div class="modal-body">
          偵測到遊戲管理員（GGM）有新版本 {{ updateInfo?.server }}（{{ updateInfo?.current ? "目前 " + updateInfo?.current : "尚未安裝" }}）。<br />
          登入功能需要最新版才能正常運作，是否現在下載並更新？
        </div>
        <div class="modal-actions">
          <button class="modal-btn" @click="updateAsk = false">稍後</button>
          <button class="modal-btn primary" @click="confirmUpdate">立即更新</button>
        </div>
      </div>
    </div>

    <ToastPop />
  </div>
</template>

<style scoped>
.app-window {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: var(--bg);
  border: 1px solid var(--edge);
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

/* ── 應用內對話框（取代原生 dialog）── */
.modal-overlay {
  position: fixed;
  inset: 0;
  z-index: 2000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(0, 0, 0, 0.42);
  backdrop-filter: blur(2px);
  -webkit-backdrop-filter: blur(2px);
}
.modal-card {
  width: 100%;
  max-width: 320px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 20px;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.35);
}
.modal-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
  margin-bottom: 10px;
}
.modal-body {
  font-size: 13px;
  line-height: 1.6;
  color: var(--text2);
  margin-bottom: 18px;
}
.modal-actions {
  display: flex;
  gap: 8px;
}
.modal-btn {
  flex: 1;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface2);
  font-size: 13px;
  font-weight: 500;
  color: var(--text2);
  transition: background 0.12s, color 0.12s;
}
.modal-btn:hover { background: var(--surface3); color: var(--text); }
.modal-btn.primary {
  background: var(--primary-bg);
  border-color: var(--primary-border);
  color: var(--primary-color);
}
.modal-btn.primary:hover { background: var(--primary-bg-hover); }
</style>

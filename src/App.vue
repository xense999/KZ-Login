<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { Window } from "@tauri-apps/api/window";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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

type SessionState = "alive" | "expired" | "unknown";

// Only "expired" clears a token. "unknown" means the ping itself failed (no
// network, server down), which says nothing about the session — treating it as
// a logout would force a QR rescan for accounts that are still perfectly alive.
async function checkSessions(): Promise<{ expired: number; unknown: number }> {
  let expired = 0;
  let unknown = 0;
  for (const acc of store.accounts) {
    if (!acc.token) continue;
    const pinged = acc.token;
    try {
      const state = await invoke<SessionState>('ping_session', { token: pinged });
      // A re-login can land mid-loop and hand this account a fresh token; the
      // verdict we are holding belongs to the token we pinged, not to that one.
      if (acc.token !== pinged) continue;
      if (state === "expired") { store.invalidateToken(acc.id); expired++; }
      else if (state === "unknown") unknown++;
    } catch { unknown++; }
  }
  return { expired, unknown };
}

const refreshing = ref(false);

// Manual refresh: F5 / Ctrl+R are swallowed by the Rust side (a real reload
// would wipe the in-memory account list) and arrive here as `refresh-sessions`.
// Re-ping every session right away so the dead ones flip back to the rescan
// state instead of failing later on 取得密碼.
async function refreshSessions() {
  if (refreshing.value) return;
  const checked = store.accounts.filter((a) => a.token);
  if (checked.length === 0) {
    toast("目前沒有已登入的帳號");
    return;
  }
  refreshing.value = true;
  toast("重新檢查登入狀態…", { ms: 15000 });
  try {
    const { expired, unknown } = await checkSessions();
    if (expired > 0) {
      toast(`${expired} 個帳號已登出，請點右側的掃碼圖示重新登入`, { kind: "error", ms: 5000 });
    } else if (unknown > 0) {
      toast("連不上伺服器，這次無法確認登入狀態", { kind: "error" });
    } else {
      toast(`${checked.length} 個帳號都還在線上`);
    }
  } finally {
    refreshing.value = false;
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

let unlistenRefresh: UnlistenFn | null = null;

onMounted(async () => {
  checkSessions();
  checkGgmUpdate();
  // Manual refresh wins: skip this tick rather than run a second ping loop over
  // the same accounts and muddle the counts the manual run reports.
  keepAliveTimer = setInterval(() => { if (!refreshing.value) checkSessions(); }, 8 * 60 * 1000);
  unlistenRefresh = await listen("refresh-sessions", () => { refreshSessions(); });
});

onUnmounted(() => {
  if (keepAliveTimer) clearInterval(keepAliveTimer);
  unlistenRefresh?.();
});

function onReauth(accountId: string) {
  reauthAccountId.value = accountId;
  page.value = "qr";
}

// Which card a scan belongs to is decided by the sub-accounts it returns: a sid
// belongs to exactly one beanfun account, so a shared sid means "this is that
// account logging in again". Without this, scanning an account already in the
// list built a second card, and nothing downstream could tell the two apart —
// beanfun supersedes the older session, but the keepalive ping reported both
// cards online (and when beanfun hands back the same bfWebToken, both cards
// really do share one live jar).
function matchExistingAccount(games: { sid: string }[]): string | null {
  const sids = new Set(games.map((g) => g.sid));
  return store.accounts.find((a) => a.gameAccounts.some((g) => sids.has(g.sid)))?.id ?? null;
}

// Free the cookie jar of a session a re-login just replaced. Only safe when the
// token string actually changed — the backend keys jars by token, so forgetting
// an unchanged token would throw away the jar the new login just stored.
async function forgetSession(token: string) {
  try {
    await invoke("forget_session", { token });
  } catch {
    // best-effort cleanup; a stale jar costs memory, never correctness
  }
}

async function onQrSuccess(token: string, games: { sn: string; sid: string; sname: string }[]) {
  const matched = matchExistingAccount(games);
  const targetId = matched ?? reauthAccountId.value;
  const wasReauth = reauthAccountId.value !== null;
  reauthAccountId.value = null;

  if (targetId) {
    const previous = store.accounts.find((a) => a.id === targetId)?.token ?? null;
    await store.updateToken(targetId, token, games);
    if (previous && previous !== token) await forgetSession(previous);
    page.value = "main";
    if (matched && !wasReauth) toast("這個帳號已經在清單中，已更新為最新的登入");
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
        <svg viewBox="0 0 24 24" fill="none" width="15" height="15">
          <circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="1.7"/>
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
            stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
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

/* .titlebar 與 .wbtn 的共通樣式在 styles/main.css（與帳號瀏覽器的殼層共用） */
.titlebar {
  height: 42px;
  padding: 0 14px;
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

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { Window } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import MainPage from "./pages/MainPage.vue";
import QrPage from "./pages/QrPage.vue";
import PasswordPage from "./pages/PasswordPage.vue";
import GamaPassPage from "./pages/GamaPassPage.vue";
import SuccessPage from "./pages/SuccessPage.vue";
import SettingsPage from "./pages/SettingsPage.vue";
import ToastPop from "./components/ToastPop.vue";
import { toast } from "./composables/useToast";
import { useAccountsStore, sameLoginAccount, type LoginMethod, type LoginResult } from "./stores/accounts";
import { useTheme } from "./composables/useTheme";
import { useMinimizeMode } from "./composables/useMinimizeMode";

type Page = "main" | "login" | "success" | "settings";

const page = ref<Page>("main");
const pendingLogin = ref<LoginResult | null>(null);
const reauthAccountId = ref<string | null>(null);
const store = useAccountsStore();

const pageTitles: Record<Page, string> = {
  main: "久世登入器",
  login: "切換登入方式",
  success: "新增帳號",
  settings: "設定",
};

// Only "add account" remembers the mode; a re-login follows that account instead.
const LOGIN_MODE_KEY = "kusei:login_mode";
const loginMode = ref<LoginMethod>("qr");
const loginPrefill = ref("");
const loginBusy = ref(false);

function openLogin(mode: LoginMethod, prefill: string) {
  loginMode.value = mode;
  loginPrefill.value = prefill;
  loginBusy.value = false;
  page.value = "login";
}

// 標題列那顆按鈕就是在這條環上走一格，所以順序也是它顯示的順序。
const LOGIN_MODES: LoginMethod[] = ["qr", "password", "gamapass"];

function savedLoginMode(): LoginMethod {
  const saved = localStorage.getItem(LOGIN_MODE_KEY);
  return LOGIN_MODES.find((m) => m === saved) ?? "qr";
}

function onAddAccount() {
  reauthAccountId.value = null;
  openLogin(savedLoginMode(), "");
}

function switchLoginMode() {
  if (loginBusy.value) return;
  const next = (LOGIN_MODES.indexOf(loginMode.value) + 1) % LOGIN_MODES.length;
  loginMode.value = LOGIN_MODES[next];
  if (!reauthAccountId.value) localStorage.setItem(LOGIN_MODE_KEY, loginMode.value);
}

function cancelLogin() {
  reauthAccountId.value = null;
  page.value = "main";
}

// 收進通知列還是一般最小化由後端照設定決定
function minimize() { invoke("minimize_main").catch(console.error); }
function close() { Window.getCurrent().close(); }

let keepAliveTimer: ReturnType<typeof setInterval> | null = null;

type SessionState = "alive" | "expired" | "unknown";

// 每 8 分鐘自己跑一次，沒有手動觸發的入口——按 F5 只會被 Rust 端吞掉。斷線的卡片
// 右邊自己變回掃碼圖示，所以這裡不出聲；沒有結論時更要安靜，那不代表任何事。
//
// Only "expired" clears a token. "unknown" means the ping itself failed (no
// network, server down), which says nothing about the session — treating it as
// a logout would force a QR rescan for accounts that are still perfectly alive.
async function checkSessions() {
  for (const acc of store.accounts) {
    if (!acc.token) continue;
    const pinged = acc.token;
    try {
      const state = await invoke<SessionState>('ping_session', { token: pinged });
      // A re-login can land mid-loop and hand this account a fresh token; the
      // verdict we are holding belongs to the token we pinged, not to that one.
      if (acc.token !== pinged) continue;
      if (state === "expired") store.invalidateToken(acc.id);
    } catch { /* 沒有結論就什麼都不做 */ }
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

onMounted(async () => {
  checkSessions();
  checkGgmUpdate();
  // The app icon follows the theme, applied once per launch — switching theme
  // mid-session only shows up next time. Fire-and-forget: it never fails, and
  // nothing here depends on it.
  invoke("apply_icon_theme", { theme: useTheme().theme.value });
  useMinimizeMode().syncMinimizeMode().catch(console.error);
  keepAliveTimer = setInterval(checkSessions, 8 * 60 * 1000);
});

onUnmounted(() => {
  if (keepAliveTimer) clearInterval(keepAliveTimer);
});

function onReauth(accountId: string) {
  const acc = store.accounts.find((a) => a.id === accountId);
  reauthAccountId.value = accountId;
  openLogin(acc?.loginMethod ?? "qr", acc?.loginAccount ?? "");
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

async function onLoginSuccess(login: LoginResult) {
  const targetId = loginTarget(login);
  reauthAccountId.value = null;

  if (targetId) {
    const previous = store.accounts.find((a) => a.id === targetId)?.token ?? null;
    await store.updateToken(targetId, login);
    if (previous && previous !== login.token) await forgetSession(previous);
    page.value = "main";
    return;
  }

  pendingLogin.value = login;
  page.value = "success";
}

// Which card a login refreshes, or null for a new card.
//
// The game accounts decide it: their serial numbers never change and belong to
// one beanfun account only, so a card holding any of them is this account's —
// however the login was made, and whichever card it was started from. Re-login
// from card A into account B refreshes B's card (or adds one) and leaves A alone.
//
// Only an account with no game accounts yet falls back to the account typed,
// and a login that carries neither (QR) to the card it was started from — unless
// that card plainly belongs to someone else: it has game accounts, this login
// has some too, and none of them match.
function loginTarget(login: LoginResult): string | null {
  const started = reauthAccountId.value ? store.accounts.find((a) => a.id === reauthAccountId.value) : undefined;
  // 同一個帳號若已經有兩張卡片（這條規則上線前留下的），從哪一張按重新登入就
  // 刷新哪一張——不然排在後面那張永遠是過期的，怎麼登都救不回來。
  const sns = new Set(login.games.map((g) => g.sn));
  if (started?.gameAccounts.some((g) => sns.has(g.sn))) return started.id;
  const owned = store.findByGames(login.games);
  if (owned) return owned.id;

  const stranger = started && started.gameAccounts.length > 0 && login.games.length > 0;
  const reauth = stranger ? undefined : started;
  if (!login.account) return reauth?.id ?? null;
  // 同一串字在 GamaPass 與 beanfun 是不同的人（見 `findByLoginAccount`），所以發起
  // 的那張卡片也要是同一邊的才算。
  const sameSide = (reauth?.loginMethod === "gamapass") === (login.method === "gamapass");
  if (sameSide && reauth?.loginAccount && sameLoginAccount(reauth.loginAccount, login.account)) return reauth.id;
  const owner = store.findByLoginAccount(login.account, login.method);
  if (owner) return owner.id;
  return reauth && reauth.loginAccount === null ? reauth.id : null;
}

function onAccountSaved() {
  pendingLogin.value = null;
  page.value = "main";
}
</script>

<template>
  <div class="app-window">
    <div class="titlebar" data-tauri-drag-region>
      <button class="wbtn settings-btn" @click="page = page === 'settings' ? 'main' : 'settings'" :class="{ active: page === 'settings' }">
        <!-- 14px 而非 15px：.wbtn 是 26×22，奇數尺寸會留下半像素邊距，捨入後圖示偏右上 -->
        <svg viewBox="0 0 24 24" fill="none" width="14" height="14">
          <circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="1.7"/>
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
            stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>
      <button v-if="page === 'login'" class="title title-btn" :disabled="loginBusy" @click="switchLoginMode">
        {{ pageTitles[page] }}
      </button>
      <span v-else class="title" data-tauri-drag-region>{{ pageTitles[page] }}</span>
      <div class="win-controls">
        <button class="wbtn" @click="minimize">&#x2212;</button>
        <button class="wbtn close" @click="close">&#x2715;</button>
      </div>
    </div>

    <div class="page-container">
      <MainPage v-if="page === 'main'" @add-account="onAddAccount" @reauth="onReauth" />
      <template v-else-if="page === 'login'">
        <QrPage v-if="loginMode === 'qr'" @cancel="cancelLogin" @success="onLoginSuccess" />
        <GamaPassPage v-else-if="loginMode === 'gamapass'" :initial-account="loginPrefill" @cancel="cancelLogin" @success="onLoginSuccess" @busy="loginBusy = $event" />
        <PasswordPage v-else :initial-account="loginPrefill" @cancel="cancelLogin" @success="onLoginSuccess" @busy="loginBusy = $event" />
      </template>
      <SuccessPage v-else-if="page === 'success'" :login="pendingLogin!" @saved="onAccountSaved" />
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

.title-btn {
  pointer-events: auto;
  padding: 3px 10px;
  border: none;
  border-radius: 7px;
  background: none;
  transition: background 0.15s, color 0.15s;
}
.title-btn:hover:not(:disabled) { background: var(--glass-hover); color: var(--text); }
.title-btn:disabled { opacity: 0.5; cursor: default; }

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
  /* 應用內遮罩（隱藏功能密鑰、批次匯出）貼著這裡：上緣是標題列那條槓，下緣是
     視窗底部，超出圓角的部分由 .app-window 的 overflow 裁掉。 */
  position: relative;
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

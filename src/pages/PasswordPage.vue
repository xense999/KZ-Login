<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useTheme } from "../composables/useTheme";
import { useAccountsStore, sameLoginAccount, type LoginGame, type LoginResult } from "../stores/accounts";

const props = defineProps<{ initialAccount: string }>();
const emit = defineEmits<{
  cancel: [];
  success: [login: LoginResult];
  busy: [busy: boolean];
}>();

type Reply =
  | { status: "approved"; token: string; games: LoginGame[] }
  | { status: "captcha" }
  | { status: "rejected"; message: string }
  | { status: "use_qr"; message: string };

const account = ref(props.initialAccount);
const password = ref("");
const showPassword = ref(false);
const phase = ref<"idle" | "submitting" | "captcha">("idle");
const notice = ref<{ kind: "error" | "info"; text: string } | null>(null);

const accountInput = ref<HTMLInputElement | null>(null);
const passwordInput = ref<HTMLInputElement | null>(null);
const accountField = ref<HTMLElement | null>(null);
const regionEl = ref<HTMLElement | null>(null);

type Saved = { account: string; password: string };
const store = useAccountsStore();
const saved = ref<Saved[]>([]);
const menuOpen = ref(false);

const busy = computed(() => phase.value !== "idle");
const canSubmit = computed(() => !busy.value && account.value.trim() !== "" && password.value !== "");

const hasCard = (name: string) => store.findByLoginAccount(name, "password") !== undefined;

onMounted(async () => {
  document.addEventListener("pointerdown", closeMenuOutside);
  try {
    saved.value = await invoke<Saved[]>("saved_logins");
  } catch (e) {
    notice.value = { kind: "error", text: String(e) };
  }
  // A re-login arrives with the card's account; its saved password, if any,
  // makes it a single click.
  if (props.initialAccount && !password.value) {
    const match = findSaved(props.initialAccount);
    if (match) password.value = match.password;
  }
  (props.initialAccount ? passwordInput : accountInput).value?.focus();
});

onUnmounted(() => document.removeEventListener("pointerdown", closeMenuOutside));

function findSaved(name: string) {
  return saved.value.find((s) => sameLoginAccount(s.account, name));
}

function closeMenuOutside(e: PointerEvent) {
  if (menuOpen.value && !accountField.value?.contains(e.target as Node)) menuOpen.value = false;
}

function pick(entry: Saved) {
  account.value = entry.account;
  password.value = entry.password;
  menuOpen.value = false;
  notice.value = null;
  passwordInput.value?.focus();
}

// Drag by the dot. The list reorders live under the pointer and is saved once
// on release.
const draggingIdx = ref<number | null>(null);

function onGripDown(e: PointerEvent, idx: number) {
  e.preventDefault();
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  draggingIdx.value = idx;
}

function onGripMove(e: PointerEvent) {
  if (draggingIdx.value === null) return;
  const row = document.elementFromPoint(e.clientX, e.clientY)?.closest("[data-saved-idx]") as HTMLElement | null;
  if (!row) return;
  const to = Number(row.dataset.savedIdx);
  const from = draggingIdx.value;
  if (to === from) return;
  const list = [...saved.value];
  const [moved] = list.splice(from, 1);
  list.splice(to, 0, moved);
  saved.value = list;
  draggingIdx.value = to;
}

async function onGripUp() {
  if (draggingIdx.value === null) return;
  draggingIdx.value = null;
  try {
    await invoke("reorder_saved_logins", { accounts: saved.value.map((s) => s.account) });
  } catch (e) {
    notice.value = { kind: "error", text: String(e) };
  }
}

async function forget(entry: Saved) {
  try {
    await invoke("forget_saved_login", { account: entry.account });
    saved.value = saved.value.filter((s) => s !== entry);
    if (saved.value.length === 0) menuOpen.value = false;
  } catch (e) {
    notice.value = { kind: "error", text: String(e) };
  }
}

function setPhase(p: typeof phase.value) {
  phase.value = p;
  emit("busy", p !== "idle");
}

// The captcha window lives outside this page, so hand it the resolved theme
// colours instead of keeping a second palette in Rust.
function readPalette() {
  const css = getComputedStyle(document.documentElement);
  const token = (name: string) => css.getPropertyValue(name).trim();
  return {
    bg: token("--bg"),
    text: token("--text"),
    dark: useTheme().theme.value === "dark",
  };
}

// Where the captcha window sits: the strip between the title bar and the
// bottom bar, in CSS pixels of the main window.
function readRegion() {
  const r = regionEl.value!.getBoundingClientRect();
  return { x: r.left, y: r.top, width: r.width, height: r.height };
}

// While the checkbox is up, the page's own cancel button closes it.
function onCancel() {
  if (phase.value === "captcha") invoke("captcha_cancel");
  else emit("cancel");
}

async function submit() {
  if (!canSubmit.value) return;
  notice.value = null;
  setPhase("submitting");
  const typed = account.value.trim();
  try {
    let reply = await invoke<Reply>("password_login_start", { account: typed, password: password.value });
    while (reply.status === "captcha") {
      setPhase("captcha");
      const captcha = await invoke<string | null>("captcha_solve", { palette: readPalette(), region: readRegion() });
      if (!captcha) {
        notice.value = { kind: "info", text: "驗證已取消或逾時，請再按一次登入" };
        return;
      }
      setPhase("submitting");
      reply = await invoke<Reply>("password_login_resume", { captcha });
    }
    if (reply.status === "approved") {
      emit("success", { token: reply.token, games: reply.games, method: "password", account: typed });
    } else if (reply.status === "rejected") {
      notice.value = { kind: "error", text: reply.message };
      passwordInput.value?.select();
    } else {
      notice.value = { kind: "error", text: `此帳號需要額外驗證，請改用 QR 登入（${reply.message}）` };
    }
  } catch (e) {
    notice.value = { kind: "error", text: String(e) };
  } finally {
    setPhase("idle");
  }
}
</script>

<template>
  <form class="pw-page" @submit.prevent="submit">
    <div ref="regionEl" class="pw-main">
      <div class="pw-hd">
        <h2>帳號密碼登入</h2>
        <p>輸入 beanfun 帳號與密碼</p>
      </div>

      <div class="pw-body">
        <div ref="accountField" class="pw-field">
          <span class="pw-dot" :class="{ on: hasCard(account) }"></span>
          <input
            ref="accountInput"
            v-model="account"
            class="pw-input with-dot"
            placeholder="帳號"
            autocomplete="off"
            spellcheck="false"
            :disabled="busy"
            @keydown.esc="menuOpen = false"
          />
          <button
            type="button"
            class="pw-icon"
            :class="{ on: menuOpen }"
            title="已儲存的帳號"
            :disabled="busy || saved.length === 0"
            @click="menuOpen = !menuOpen"
          >
            <svg viewBox="0 0 16 16" fill="none" width="12" height="12">
              <path d="M4 6l4 4 4-4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </button>
          <ul v-if="menuOpen" class="pw-menu">
            <li
              v-for="(entry, idx) in saved"
              :key="entry.account"
              :data-saved-idx="idx"
              class="pw-row"
              :class="{ dragging: draggingIdx === idx }"
              @click="pick(entry)"
            >
              <span
                class="pw-grip"
                title="拖移排序"
                @pointerdown="onGripDown($event, idx)"
                @pointermove="onGripMove"
                @pointerup="onGripUp"
                @pointercancel="onGripUp"
                @click.stop
              >
                <span class="pw-dot" :class="{ on: hasCard(entry.account) }"></span>
              </span>
              <span class="pw-row-name">{{ entry.account }}</span>
              <button type="button" class="pw-row-del" title="刪除這組帳密" @click.stop="forget(entry)">✕</button>
            </li>
          </ul>
        </div>
        <div class="pw-field">
          <input
            ref="passwordInput"
            v-model="password"
            class="pw-input"
            :type="showPassword ? 'text' : 'password'"
            placeholder="密碼"
            autocomplete="current-password"
            :disabled="busy"
          />
          <button
            type="button"
            class="pw-icon"
            :class="{ on: showPassword }"
            :title="showPassword ? '隱藏密碼' : '顯示密碼'"
            :disabled="busy"
            @click="showPassword = !showPassword"
          >
            <svg viewBox="0 0 24 24" fill="none" width="16" height="16">
              <path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
              <circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="1.6"/>
              <path v-if="!showPassword" d="M4 20L20 4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
            </svg>
          </button>
        </div>
        <div class="pw-notice" :class="notice?.kind">
          <template v-if="phase === 'captcha'">請在驗證畫面勾選「我不是機器人」</template>
          <template v-else-if="phase === 'submitting'">登入中…</template>
          <template v-else-if="notice">{{ notice.text }}</template>
        </div>
      </div>

    </div>

    <div class="bottom-bar">
      <button type="button" class="btn-ghost" :disabled="phase === 'submitting'" @click="onCancel">取消</button>
      <button type="submit" class="btn-solid" :disabled="!canSubmit">
        <span v-if="busy" class="spin"></span>
        <template v-else>登入</template>
      </button>
    </div>
  </form>
</template>

<style scoped>
.pw-page { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.pw-main {
  display: flex; flex-direction: column; align-items: center;
  gap: 18px; padding: 22px 18px 18px; flex: 1; min-height: 0;
}

.pw-hd { text-align: center; }
.pw-hd h2 { font-size: 16px; font-weight: 600; color: var(--text); letter-spacing: -0.01em; }
.pw-hd p  { font-size: 13px; color: var(--text2); margin-top: 4px; }

.pw-body {
  flex: 1; display: flex; flex-direction: column; justify-content: center;
  gap: 10px; width: 100%; max-width: 300px;
}

.pw-field { position: relative; }

.pw-input {
  width: 100%;
  background: var(--input-bg);
  border: 1px solid var(--input-border);
  border-radius: 10px;
  padding: 11px 12px;
  font-size: 13px;
  color: var(--text);
  outline: none;
  transition: border-color 0.15s;
}
.pw-field .pw-input { padding-right: 40px; }
.pw-input::placeholder { color: var(--text3); }
.pw-input:focus { border-color: var(--primary-border); }
.pw-input:disabled { opacity: 0.6; }

.pw-field .pw-input.with-dot { padding-left: 28px; }

.pw-icon {
  position: absolute; top: 50%; right: 6px; transform: translateY(-50%);
  width: 28px; height: 28px; border: none; border-radius: 7px;
  background: none; color: var(--text3);
  transition: background 0.15s, color 0.15s;
}
.pw-icon:hover:not(:disabled), .pw-icon.on { color: var(--text2); background: var(--glass-hover); }
.pw-icon:disabled { opacity: 0.35; cursor: default; }

.pw-dot {
  width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0;
  background: var(--text3); opacity: 0.55;
}
.pw-dot.on { background: var(--green); opacity: 1; }
.pw-field > .pw-dot {
  position: absolute; left: 12px; top: 50%; transform: translateY(-50%);
  pointer-events: none;
}

.pw-menu {
  position: absolute; top: calc(100% + 4px); left: 0; right: 0; z-index: 10;
  list-style: none; padding: 4px;
  max-height: 180px; overflow-y: auto;
  background: var(--ctx-menu-bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: var(--ctx-shadow);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
}
.pw-row {
  display: flex; align-items: center; gap: 9px;
  padding: 7px 6px 7px 8px; border-radius: 7px;
  font-size: 13px; color: var(--text); cursor: pointer;
}
.pw-row:hover { background: var(--ctx-hover); }
.pw-row.dragging { background: var(--ctx-hover); }

.pw-grip {
  display: flex; align-items: center; justify-content: center;
  width: 20px; height: 22px; margin: -4px -4px -4px -6px;
  cursor: grab; touch-action: none; border-radius: 5px;
}
.pw-grip:hover { background: var(--glass-hover); }
.pw-grip:active { cursor: grabbing; }
.pw-row-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.pw-row-del {
  width: 22px; height: 22px; border: none; border-radius: 6px;
  background: none; color: var(--text3); font-size: 11px;
}
.pw-row-del:hover { background: rgba(255,69,58,0.15); color: var(--red); }

.pw-notice {
  min-height: 38px;
  font-size: 12px; line-height: 1.6;
  color: var(--text2);
  word-break: break-all;
}
.pw-notice.error { color: var(--red); }

</style>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
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

type Saved = { account: string; password: string };

// 上次用哪一組要自己記：`credentials` 的清單順序是使用者排的，已存在的帳號再次
// 登入不會移位，拿最後一筆當「最近用的」會挑到別人（連密碼一起）。
const LAST_KEY = "kusei:gamapass_last";

const account = ref("");
const password = ref("");
const saved = ref<Saved[]>([]);
const menuOpen = ref(false);
const accountField = ref<HTMLElement | null>(null);
const running = ref(false);
const errorMsg = ref("");
// 切走時這一頁會被卸載，但視窗可能還開著；那時候的回覆不該再動這一頁。
let disposed = false;

// 記住的帳密：最後用的那組直接填好，其他的收在下拉裡（同帳密登入頁）。
onMounted(async () => {
  document.addEventListener("pointerdown", closeMenuOutside);
  try {
    saved.value = await invoke<Saved[]>("saved_gamapass");
    if (disposed) return;
    const wanted = localStorage.getItem(LAST_KEY);
    const last = saved.value.find((s) => s.account === wanted) ?? saved.value[saved.value.length - 1];
    if (last) pick(last);
  } catch { /* 沒記住就空著 */ }
});

// 3：點到別處就收起來。選單蓋在密碼欄上，不收的話「點密碼欄」會點到選單的某一列，
// 帳密就被無聲換掉。
function closeMenuOutside(e: PointerEvent) {
  if (menuOpen.value && !accountField.value?.contains(e.target as Node)) menuOpen.value = false;
}

onUnmounted(() => {
  disposed = true;
  document.removeEventListener("pointerdown", closeMenuOutside);
  invoke("gamapass_cancel").catch(() => { /* 視窗早就關了 */ });
});

const hasAccount = computed(() => account.value.trim().length > 0);
const canLogin = computed(() => hasAccount.value && password.value.length > 0);

function pick(entry: Saved) {
  account.value = entry.account;
  password.value = entry.password;
  menuOpen.value = false;
}

async function forget(entry: Saved) {
  try {
    // 真的刪掉了才從畫面上拿掉：刪失敗卻讓它消失，下次進來又冒出來，使用者
    // 會以為自己刪的沒生效——事實上它從來就沒被刪掉。
    await invoke("forget_gamapass", { account: entry.account });
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : String(e);
    return;
  }
  saved.value = saved.value.filter((s) => s !== entry);
  if (saved.value.length === 0) menuOpen.value = false;
  // 刪掉的正是欄位裡那組，就把欄位也清掉——留著會讓人以為它還記著。
  if (entry.account === account.value) {
    account.value = "";
    password.value = "";
  }
}

// passkey 一樣帶帳號過去（不然使用者要在對方頁面重打一次），只是不帶密碼：
// 帳號填完、過了那一步就把視窗交給他，因為 passkey 的憑證綁在對方網域上，
// 只有他們自己的頁面問得到。
async function run(withPassword: boolean) {
  if (withPassword ? !canLogin.value : !hasAccount.value) return;
  errorMsg.value = "";
  menuOpen.value = false;
  running.value = true;
  emit("busy", true);
  try {
    const result = await invoke<Result>("gamapass_login", {
      account: account.value.trim(),
      password: withPassword ? password.value : null,
    });
    if (disposed) return;
    if (result.status === "approved") {
      try { localStorage.setItem(LAST_KEY, account.value.trim()); } catch { /* 記不住就算了 */ }
      emit("success", {
        token: result.token,
        games: result.games,
        method: "gamapass",
        // 帳號是打進對方頁面的，不是我們的表單狀態能代表的登入身分（同 QR）。
        account: null,
      });
      return;
    }
  } catch (e: unknown) {
    if (!disposed) errorMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (!disposed) {
      running.value = false;
      emit("busy", false);
    }
  }
}

function onCancel() {
  if (running.value) invoke("gamapass_cancel").catch(() => { /* 已經關了 */ });
  else emit("cancel");
}
</script>

<template>
  <div class="gp-page">
    <div class="gp-main">
      <template v-if="running">
        <div class="spinner-lg"></div>
        <span class="status-txt">登入中…</span>
        <span class="hint">登入視窗會跳出來，可以看著它把你填的東西打進去。</span>
      </template>

      <template v-else>
        <div class="gp-hd">
          <h2>GamaPass 登入</h2>
          <p>用遊戲橘子的帳號登入</p>
        </div>

        <div class="form">
          <div ref="accountField" class="field-wrap">
            <input
              v-model="account"
              class="field"
              type="text"
              inputmode="email"
              autocomplete="off"
              spellcheck="false"
              placeholder="手機號碼或電子郵件"
              @keydown.esc="menuOpen = false"
            />
            <button
              type="button"
              class="field-icon"
              :class="{ on: menuOpen }"
              title="已儲存的帳號"
              :disabled="saved.length === 0"
              @click="menuOpen = !menuOpen"
            >
              <svg viewBox="0 0 16 16" fill="none" width="12" height="12">
                <path d="M4 6l4 4 4-4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
              </svg>
            </button>
            <ul v-if="menuOpen" class="menu">
              <li v-for="entry in saved" :key="entry.account" class="menu-row" @click="pick(entry)">
                <span class="menu-name">{{ entry.account }}</span>
                <button type="button" class="menu-del" title="刪除這組帳密" @click.stop="forget(entry)">✕</button>
              </li>
            </ul>
          </div>
          <input
            v-model="password"
            class="field"
            type="password"
            autocomplete="off"
            placeholder="密碼"
            @keyup.enter="run(true)"
          />
          <div v-if="errorMsg" class="err">{{ errorMsg }}</div>
        </div>

        <button class="btn-passkey" :disabled="!hasAccount" @click="run(false)">改用 passkey（不用密碼）</button>
      </template>
    </div>

    <div class="bottom-bar">
      <button class="btn-ghost" @click="onCancel">取消</button>
      <button v-if="!running" class="btn-solid" :disabled="!canLogin" @click="run(true)">登入</button>
    </div>
  </div>
</template>

<style scoped>
.gp-page { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.gp-main {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  gap: 16px; padding: 22px 18px 18px; flex: 1; min-height: 0;
}

.gp-hd { text-align: center; }
.gp-hd h2 { font-size: 16px; font-weight: 600; color: var(--text); letter-spacing: -0.01em; }
.gp-hd p  { font-size: 13px; color: var(--text2); margin-top: 4px; }

.form { display: flex; flex-direction: column; gap: 8px; width: 100%; max-width: 280px; }
.field-wrap { position: relative; }
.field-wrap .field { padding-right: 40px; }
.field {
  width: 100%;
  padding: 11px 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
  font-size: 13px;
  color: var(--text);
}
.field:focus { outline: none; border-color: var(--primary-border); }
.err { font-size: 12px; color: var(--red); line-height: 1.6; }

.field-icon {
  position: absolute; top: 50%; right: 6px; transform: translateY(-50%);
  width: 28px; height: 28px; border: none; border-radius: 7px;
  background: none; color: var(--text3);
  transition: background 0.15s, color 0.15s;
}
.field-icon:hover:not(:disabled), .field-icon.on { color: var(--text2); background: var(--glass-hover); }
.field-icon:disabled { opacity: 0.35; cursor: default; }

.menu {
  position: absolute; top: calc(100% + 4px); left: 0; right: 0; z-index: 10;
  list-style: none; padding: 4px; margin: 0;
  max-height: 180px; overflow-y: auto;
  background: var(--ctx-menu-bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: var(--ctx-shadow);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
}
.menu-row {
  display: flex; align-items: center; gap: 9px;
  padding: 7px 6px 7px 10px; border-radius: 7px;
  font-size: 13px; color: var(--text); cursor: pointer;
}
.menu-row:hover { background: var(--ctx-hover); }
.menu-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.menu-del {
  flex-shrink: 0; width: 22px; height: 22px;
  border: none; border-radius: 6px; background: none;
  font-size: 11px; color: var(--text3);
}
.menu-del:hover { background: rgba(255,69,58,0.15); color: var(--red); }

.extras { display: flex; flex-direction: column; align-items: center; gap: 10px; }
.btn-passkey {
  padding: 9px 16px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: none;
  font-size: 12px;
  color: var(--text2);
  transition: background 0.15s, color 0.15s;
}
.btn-passkey:hover:not(:disabled) { background: var(--surface2); color: var(--text); }
.btn-passkey:disabled { opacity: 0.4; cursor: default; }
.link {
  padding: 0; border: none; background: none;
  font-size: 12px; color: var(--text3); text-decoration: underline;
}
.link:hover { color: var(--text2); }

.status-txt { font-size: 13px; color: var(--text2); }
.hint { font-size: 12px; color: var(--text3); text-align: center; max-width: 240px; line-height: 1.6; }

/* 同掃碼頁的等待指示器（scoped 樣式各自為政，共用的只有 main.css 的 token）。 */
.spinner-lg {
  width: 32px; height: 32px;
  border: 2px solid rgba(255,255,255,0.07);
  border-top-color: rgba(255,255,255,0.5);
  border-radius: 50%; animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }
</style>

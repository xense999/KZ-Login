<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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

// 登入在一顆看不見的視窗裡進行；它走到哪裡，後端用這個事件告訴我們。
type Stage =
  | { stage: "working" }
  | { stage: "code"; sentTo: string; error: string; attempt: number }
  | { stage: "user" };

// 上次用哪一組要自己記：`credentials` 的清單順序是使用者排的，已存在的帳號再次
// 登入不會移位，拿最後一筆當「最近用的」會挑到別人。
const LAST_KEY = "kusei:gamapass_last";
// 對方的驗證碼固定四位數。
const CODE_LENGTH = 4;
// 送出驗證碼後等這麼久還沒有下文，就讓使用者再打一次：後端只在狀態「變了」的
// 時候才通知，而對方頁面當下若沒收下那串數字，狀態就不會變。
const CODE_REPLY_MS = 8000;

const saved = ref<Saved[]>([]);
const chosen = ref<Saved | null>(null);
const menuOpen = ref(false);
const pickerEl = ref<HTMLElement | null>(null);
const regionEl = ref<HTMLElement | null>(null);

const account = ref("");
const password = ref("");

const running = ref(false);
const stage = ref<Stage>({ stage: "working" });
const code = ref("");
const codeSent = ref(false);
const codeInput = ref<HTMLInputElement | null>(null);
const errorMsg = ref("");
// 切走時這一頁會被卸載，但登入可能還在跑；那時候的回覆不該再動這一頁。
let disposed = false;
let unlisten: UnlistenFn | null = null;
let codeTimer: number | undefined;

onMounted(async () => {
  document.addEventListener("pointerdown", closeMenuOutside);
  const stop = await listen<Stage>("gamapass-stage", (e) => onStage(e.payload));
  if (disposed) { stop(); return; }
  unlisten = stop;
  await loadSaved();
});

async function loadSaved() {
  try {
    saved.value = await invoke<Saved[]>("saved_gamapass");
    if (disposed) return;
    const wanted = localStorage.getItem(LAST_KEY);
    chosen.value = saved.value.find((s) => s.account === wanted) ?? saved.value[saved.value.length - 1] ?? null;
  } catch { /* 沒記住就只剩新增 */ }
}

// 點到別處就收起來：選單蓋在下面的欄位上，不收的話點欄位會點到選單的某一列。
function closeMenuOutside(e: PointerEvent) {
  if (menuOpen.value && !pickerEl.value?.contains(e.target as Node)) menuOpen.value = false;
}

onUnmounted(() => {
  disposed = true;
  unlisten?.();
  clearTimeout(codeTimer);
  document.removeEventListener("pointerdown", closeMenuOutside);
  invoke("gamapass_cancel").catch(() => { /* 早就結束了 */ });
});

const canAdd = computed(() => account.value.trim().length > 0 && password.value.length > 0);

function pick(entry: Saved) {
  chosen.value = entry;
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
  if (chosen.value === entry) chosen.value = saved.value[saved.value.length - 1] ?? null;
  if (saved.value.length === 0) menuOpen.value = false;
}

function onStage(next: Stage) {
  if (disposed || !running.value) return;
  clearTimeout(codeTimer);
  stage.value = next;
  if (next.stage === "code") {
    code.value = "";
    codeSent.value = false;
    nextTick(() => codeInput.value?.focus());
  }
}

// 登入視窗需要人的時候會貼在這一塊上：標題列與底部按鈕列之間，主視窗的 CSS px。
function readRegion() {
  const r = regionEl.value!.getBoundingClientRect();
  return { x: r.left, y: r.top, width: r.width, height: r.height };
}

// `fresh`：新增的帳號。它的密碼還沒被對方驗過，所以後端不走「點記住的帳號」那條
// 捷徑——那條不看密碼，打錯的密碼也會登入成功、然後被我們存起來。
async function login(entry: Saved, fresh: boolean) {
  if (running.value) return;
  errorMsg.value = "";
  menuOpen.value = false;
  stage.value = { stage: "working" };
  running.value = true;
  emit("busy", true);
  try {
    const result = await invoke<Result>("gamapass_login", {
      account: entry.account,
      password: entry.password,
      fresh,
      region: readRegion(),
    });
    if (disposed) return;
    if (result.status === "approved") {
      try { localStorage.setItem(LAST_KEY, entry.account); } catch { /* 記不住就算了 */ }
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
    clearTimeout(codeTimer);
    if (!disposed) {
      running.value = false;
      emit("busy", false);
    }
  }
}

function addAndLogin() {
  if (canAdd.value) login({ account: account.value.trim(), password: password.value }, true);
}

// 送出後等後端的下一個事件：收了就回到「登入中」，不收就再問一次並帶著對方的說法。
async function sendCode() {
  const digits = code.value.replace(/\D/g, "");
  if (digits.length !== CODE_LENGTH || codeSent.value) return;
  codeSent.value = true;
  errorMsg.value = "";
  clearTimeout(codeTimer);
  codeTimer = window.setTimeout(() => {
    if (disposed || !codeSent.value) return;
    code.value = "";
    codeSent.value = false;
    errorMsg.value = "沒有收到回應，請再輸入一次";
    nextTick(() => codeInput.value?.focus());
  }, CODE_REPLY_MS);
  try {
    await invoke("gamapass_code", { code: digits });
  } catch (e) {
    clearTimeout(codeTimer);
    codeSent.value = false;
    errorMsg.value = e instanceof Error ? e.message : String(e);
  }
}

function onCancel() {
  if (running.value) invoke("gamapass_cancel").catch(() => { /* 已經結束了 */ });
  else emit("cancel");
}
</script>

<template>
  <div class="gp-page">
    <div ref="regionEl" class="gp-main">
      <template v-if="running">
        <template v-if="stage.stage === 'code'">
          <div class="gp-hd">
            <h2>輸入驗證碼</h2>
            <p>{{ stage.sentTo ? `驗證碼已傳送至 ${stage.sentTo}` : "遊戲橘子傳了一組驗證碼給你" }}</p>
          </div>
          <div class="form">
            <input
              ref="codeInput"
              v-model="code"
              class="field code"
              type="text"
              inputmode="numeric"
              autocomplete="one-time-code"
              :maxlength="CODE_LENGTH"
              :disabled="codeSent"
              @input="sendCode"
            />
            <div v-if="codeSent" class="hint">確認中…</div>
            <div v-else-if="stage.error" class="err">{{ stage.error }}</div>
            <div v-else-if="errorMsg" class="err">{{ errorMsg }}</div>
          </div>
        </template>

        <!-- 登入視窗會蓋在這一塊上；這行字只在它還沒貼上來的那一瞬間看得到。 -->
        <span v-else-if="stage.stage === 'user'" class="status-txt">請在畫面中完成這一步</span>

        <template v-else>
          <div class="spinner-lg"></div>
          <span class="status-txt">登入中…</span>
        </template>
      </template>

      <template v-else>
        <div class="gp-hd">
          <h2>GamaPass 登入</h2>
          <p>用遊戲橘子的帳號登入</p>
        </div>

        <div v-if="saved.length" class="form">
          <div class="label">已記住的帳號</div>
          <div class="row">
            <div ref="pickerEl" class="field-wrap grow">
              <button type="button" class="field picker" :class="{ on: menuOpen }" @click="menuOpen = !menuOpen">
                <span class="menu-name">{{ chosen?.account ?? "選擇帳號" }}</span>
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
            <button class="btn-solid" :disabled="!chosen" @click="chosen && login(chosen, false)">登入</button>
          </div>
        </div>

        <div class="form">
          <div class="label">新增帳號</div>
          <input
            v-model="account"
            class="field"
            type="text"
            inputmode="email"
            autocomplete="off"
            spellcheck="false"
            placeholder="手機號碼或電子郵件"
          />
          <input
            v-model="password"
            class="field"
            type="password"
            autocomplete="off"
            placeholder="密碼"
            @keyup.enter="addAndLogin"
          />
          <button class="btn-add" :disabled="!canAdd" @click="addAndLogin">新增並登入</button>
          <div v-if="errorMsg" class="err">{{ errorMsg }}</div>
          <div class="hint left">
            第一次登入可能要輸入驗證碼，之後選帳號就能直接登入。帳號若開了「優先使用 Passkey」，請先到 gamapass 會員中心關掉，否則每次都要驗證。
          </div>
        </div>
      </template>
    </div>

    <div class="bottom-bar">
      <button class="btn-ghost" @click="onCancel">取消</button>
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
.label { font-size: 12px; color: var(--text3); }
.row { display: flex; gap: 8px; align-items: stretch; }
.grow { flex: 1; min-width: 0; }
.field-wrap { position: relative; }
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

.picker { display: flex; align-items: center; gap: 8px; text-align: left; }
.picker svg { flex-shrink: 0; color: var(--text3); }
.picker.on { border-color: var(--primary-border); }
/* 字距會加在最後一個字後面，左邊補同樣的寬度才置中。 */
.code { text-align: center; font-size: 20px; letter-spacing: 0.5em; padding-left: calc(12px + 0.5em); }

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

.btn-add {
  padding: 10px 16px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: none;
  font-size: 13px;
  color: var(--text2);
  transition: background 0.15s, color 0.15s;
}
.btn-add:hover:not(:disabled) { background: var(--surface2); color: var(--text); }
.btn-add:disabled { opacity: 0.4; cursor: default; }

.status-txt { font-size: 13px; color: var(--text2); }
.hint { font-size: 12px; color: var(--text3); text-align: center; line-height: 1.6; }
.hint.left { text-align: left; }

/* 同掃碼頁的等待指示器（scoped 樣式各自為政，共用的只有 main.css 的 token）。 */
.spinner-lg {
  width: 32px; height: 32px;
  border: 2px solid rgba(255,255,255,0.07);
  border-top-color: rgba(255,255,255,0.5);
  border-radius: 50%; animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }
</style>

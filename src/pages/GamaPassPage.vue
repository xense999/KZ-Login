<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { LoginGame, LoginResult } from "../stores/accounts";

const emit = defineEmits<{
  cancel: [];
  success: [login: LoginResult];
}>();

type Result =
  | { status: "approved"; token: string; games: LoginGame[] }
  | { status: "cancelled" };

// 帳號、密碼、登入中。帳號與密碼分兩步問，跟 GamaPass 自己的順序一致。
type Step = "account" | "password" | "running";

const step = ref<Step>("account");
const account = ref("");
const password = ref("");
const errorMsg = ref("");
// 切走時這一頁會被卸載，但視窗可能還開著；那時候的回覆不該再動這一頁。
let disposed = false;

// 上次登入成功記住的那組，直接帶到密碼那一步，按登入就好。
onMounted(async () => {
  try {
    const saved = await invoke<{ account: string; password: string } | null>("saved_gamapass");
    if (disposed || !saved || account.value) return;
    account.value = saved.account;
    password.value = saved.password;
    step.value = "password";
  } catch { /* 沒記住就從頭問起 */ }
});

async function forget() {
  const who = account.value.trim();
  password.value = "";
  account.value = "";
  step.value = "account";
  try { await invoke("forget_gamapass", { account: who }); } catch { /* 沒存過也無妨 */ }
}

onUnmounted(() => {
  disposed = true;
  invoke("gamapass_cancel").catch(() => { /* 視窗早就關了 */ });
});

const canNext = computed(() => account.value.trim().length > 0);
const canLogin = computed(() => password.value.length > 0);

function toPassword() {
  if (!canNext.value) return;
  errorMsg.value = "";
  step.value = "password";
}

// passkey 一樣帶帳號過去（不然使用者要在對方頁面重打一次），只是不帶密碼：
// 帳號填完、過了那一步就把視窗交給他，因為 passkey 的憑證綁在對方網域上，
// 只有他們自己的頁面問得到。
async function run(withPassword: boolean) {
  if (withPassword && !canLogin.value) return;
  errorMsg.value = "";
  step.value = "running";
  try {
    const result = await invoke<Result>("gamapass_login", {
      account: account.value.trim(),
      password: withPassword ? password.value : null,
    });
    if (disposed) return;
    if (result.status === "approved") {
      emit("success", {
        token: result.token,
        games: result.games,
        method: "gamapass",
        // 帳號是打進對方頁面的，不是我們的表單狀態能代表的登入身分（同 QR）。
        account: null,
      });
      return;
    }
    // 視窗關掉了：回到密碼那一步，帳號留著，不用從頭打。
    step.value = "password";
  } catch (e: unknown) {
    if (disposed) return;
    errorMsg.value = e instanceof Error ? e.message : String(e);
    step.value = "password";
  }
}

function onCancel() {
  if (step.value === "running") invoke("gamapass_cancel").catch(() => { /* 已經關了 */ });
  else if (step.value === "password") step.value = "account";
  else emit("cancel");
}
</script>

<template>
  <div class="gp-page">
    <div class="gp-main">
      <template v-if="step === 'running'">
        <div class="spinner-lg"></div>
        <span class="status-txt">登入中…</span>
        <span class="hint">需要你確認的時候會另外開一個視窗。</span>
      </template>

      <template v-else>
        <div class="gp-hd">
          <h2>GamaPass 登入</h2>
          <p>{{ step === "account" ? "輸入手機號碼或電子郵件" : "輸入密碼" }}</p>
        </div>

        <div class="form">
          <input
            v-if="step === 'account'"
            v-model="account"
            class="field"
            type="text"
            inputmode="email"
            autocomplete="off"
            spellcheck="false"
            placeholder="手機號碼或電子郵件"
            @keyup.enter="toPassword"
          />
          <template v-else>
            <div class="who">
              {{ account }}
              <button class="link" @click="forget">換一組</button>
            </div>
            <input
              v-model="password"
              class="field"
              type="password"
              autocomplete="off"
              placeholder="密碼"
              @keyup.enter="run(true)"
            />
          </template>

          <div v-if="errorMsg" class="err">{{ errorMsg }}</div>
        </div>

        <button v-if="step === 'password'" class="btn-passkey" @click="run(false)">使用 passkey</button>
      </template>
    </div>

    <div class="bottom-bar">
      <button class="btn-ghost" @click="onCancel">{{ step === "password" ? "上一步" : "取消" }}</button>
      <button v-if="step === 'account'" class="btn-solid" :disabled="!canNext" @click="toPassword">下一步</button>
      <button v-else-if="step === 'password'" class="btn-solid" :disabled="!canLogin" @click="run(true)">登入</button>
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
.who {
  display: flex; align-items: center; justify-content: center; gap: 6px;
  font-size: 12px; color: var(--text3);
}
.link {
  padding: 0; border: none; background: none;
  font-size: 12px; color: var(--primary-color); text-decoration: underline;
}
.status-txt { font-size: 13px; color: var(--text2); }
.hint { font-size: 12px; color: var(--text3); text-align: center; max-width: 240px; line-height: 1.6; }
.err { font-size: 12px; color: var(--red); line-height: 1.6; }

.btn-passkey {
  padding: 9px 16px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: none;
  font-size: 12px;
  color: var(--text2);
  transition: background 0.15s, color 0.15s;
}
.btn-passkey:hover { background: var(--surface2); color: var(--text); }

/* 同掃碼頁的等待指示器（scoped 樣式各自為政，共用的只有 main.css 的 token）。 */
.spinner-lg {
  width: 32px; height: 32px;
  border: 2px solid rgba(255,255,255,0.07);
  border-top-color: rgba(255,255,255,0.5);
  border-radius: 50%; animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }
</style>

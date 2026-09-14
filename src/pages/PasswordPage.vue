<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useTheme } from "../composables/useTheme";
import type { LoginResult } from "../stores/accounts";

const props = defineProps<{ initialAccount: string }>();
const emit = defineEmits<{
  cancel: [];
  success: [login: LoginResult];
  busy: [busy: boolean];
}>();

type Games = LoginResult["games"];
type Reply =
  | { status: "approved"; token: string; games: Games }
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

const busy = computed(() => phase.value !== "idle");
const canSubmit = computed(() => !busy.value && account.value.trim() !== "" && password.value !== "");

onMounted(() => {
  (props.initialAccount ? passwordInput : accountInput).value?.focus();
});

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
    surface: token("--surface"),
    text: token("--text"),
    border: token("--border"),
    dark: useTheme().theme.value === "dark",
  };
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
      const captcha = await invoke<string | null>("captcha_solve", { palette: readPalette() });
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
    <div class="pw-hd">
      <h2>帳號密碼登入</h2>
      <p>輸入 beanfun 帳號與密碼</p>
    </div>

    <div class="pw-body">
      <input
        ref="accountInput"
        v-model="account"
        class="pw-input"
        placeholder="帳號"
        autocomplete="username"
        spellcheck="false"
        :disabled="busy"
      />
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
          class="pw-eye"
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

    <div class="login-actions">
      <button type="button" class="btn-ghost" :disabled="busy" @click="$emit('cancel')">取消</button>
      <button type="submit" class="btn-solid" :disabled="!canSubmit">
        <span v-if="busy" class="pw-spin"></span>
        <template v-else>登入</template>
      </button>
    </div>
  </form>
</template>

<style scoped>
.pw-page {
  display: flex; flex-direction: column; align-items: center;
  gap: 18px; padding: 22px 18px 18px; flex: 1;
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

.pw-eye {
  position: absolute; top: 50%; right: 6px; transform: translateY(-50%);
  width: 28px; height: 28px; border: none; border-radius: 7px;
  background: none; color: var(--text3);
  transition: background 0.15s, color 0.15s;
}
.pw-eye:hover:not(:disabled), .pw-eye.on { color: var(--text2); background: var(--glass-hover); }

.pw-notice {
  min-height: 38px;
  font-size: 12px; line-height: 1.6;
  color: var(--text2);
  word-break: break-all;
}
.pw-notice.error { color: var(--red); }

.pw-spin {
  width: 14px; height: 14px;
  border: 2px solid var(--spin-track);
  border-top-color: var(--primary-color);
  border-radius: 50%; animation: pw-rot 0.8s linear infinite;
}
@keyframes pw-rot { to { transform: rotate(360deg); } }
</style>

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { getVersion } from "@tauri-apps/api/app";
import { toast } from "../composables/useToast";
import { useTheme } from "../composables/useTheme";

const AUTHOR_DISCORD = "xense999";
const GITHUB_URL = "https://github.com/xense999";

const emit = defineEmits<{ back: [] }>();

const { theme, setTheme } = useTheme();

const WEBHOOK_KEY = "kusei:discord_webhook";

const webhookUrl = ref("");
const gamePath = ref("");
const saved = ref(false);

const showAbout = ref(false);
const appVersion = ref("");
const discordCopied = ref(false);

onMounted(async () => {
  webhookUrl.value = localStorage.getItem(WEBHOOK_KEY) ?? "";
  try { gamePath.value = await invoke<string>("get_game_path"); } catch { /* ignore */ }
  try { appVersion.value = await getVersion(); } catch { /* ignore */ }
});

// Discord 沒有「用帳號加好友」的深層連結（深連結要數字 user ID，不是帳號），
// 所以點一下就複製帳號並就地顯示「已複製」，使用者到 Discord 搜尋帳號即可加。
async function contactDiscord() {
  try {
    await writeText(AUTHOR_DISCORD);
    discordCopied.value = true;
    setTimeout(() => { discordCopied.value = false; }, 2000);
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), { kind: "error" });
  }
}

async function openGithub() {
  try { await invoke("open_url", { url: GITHUB_URL }); } catch { /* ignore */ }
}

type UpdateInfo = { current: string; latest: string; has_update: boolean; url: string; notes: string };
type UpdateState = "idle" | "checking" | "latest" | "available" | "downloading";

const updateState = ref<UpdateState>("idle");
const latestVersion = ref("");
const updateUrl = ref("");

const updateBtnText = computed(() => {
  switch (updateState.value) {
    case "checking": return "檢查中…";
    case "latest": return "已是最新版";
    case "available": return `更新到 v${latestVersion.value}`;
    case "downloading": return "下載中…";
    default: return "檢查更新";
  }
});

// 一顆按鈕兩段：先「檢查更新」；查到新版後變成「更新到 vX」，再點一次才下載安裝。
async function onUpdateClick() {
  if (updateState.value === "available") return runUpdate();
  if (updateState.value === "checking" || updateState.value === "downloading") return;

  updateState.value = "checking";
  try {
    const u = await invoke<UpdateInfo>("check_app_update");
    if (u.has_update && u.url) {
      latestVersion.value = u.latest;
      updateUrl.value = u.url;
      updateState.value = "available";
    } else {
      updateState.value = "latest";
      setTimeout(() => { if (updateState.value === "latest") updateState.value = "idle"; }, 2500);
    }
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), { kind: "error" });
    updateState.value = "idle";
  }
}

async function runUpdate() {
  updateState.value = "downloading";
  try {
    await invoke("update_app", { url: updateUrl.value });
    toast("已開始下載並開啟安裝程式，請依畫面指示完成更新", { ms: 4000 });
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), { kind: "error" });
    updateState.value = "available";
  }
}

async function browse() {
  const sel = await open({
    multiple: false,
    filters: [{ name: "MapleStory.exe", extensions: ["exe"] }],
  });
  if (typeof sel === "string") gamePath.value = sel;
}

async function save() {
  const wh = webhookUrl.value.trim();
  if (wh) localStorage.setItem(WEBHOOK_KEY, wh);
  else localStorage.removeItem(WEBHOOK_KEY);

  const gp = gamePath.value.trim();
  if (gp) {
    try {
      await invoke("set_game_path", { path: gp });
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), { kind: "error" });
      return;
    }
  }

  saved.value = true;
  setTimeout(() => emit('back'), 500);
}

async function supportAuthor() {
  try {
    await invoke("open_url", { url: "https://portaly.cc/xense999/support" });
  } catch (e) {
    console.error(e);
  }
}
</script>

<template>
  <div class="settings-page">

    <div class="scroll-area">
      <div class="card">
        <div class="row">
          <span class="row-title">主題</span>
          <div class="seg">
            <button :class="{ active: theme === 'neutral' }" @click="setTheme('neutral')">亮色</button>
            <button :class="{ active: theme === 'dark' }" @click="setTheme('dark')">暗色</button>
          </div>
        </div>
      </div>

      <div class="card">
        <div class="row col">
          <span class="row-title" title="設定後，登入器可把登入連結自動傳到你的 Discord 頻道。&#10;・QR 登入頁按「連結版本」→ 會把登入網址傳到頻道，方便在手機或其他裝置點開登入。&#10;設定方式：Discord 頻道 → 編輯頻道 → 整合 → Webhook → 建立，複製網址貼到下方欄位。">通知設定</span>
        </div>
        <div class="row-sep"></div>
        <div class="path-row">
          <input
            v-model="webhookUrl"
            class="path-input"
            placeholder="https://discord.com/api/webhooks/..."
            spellcheck="false"
            title="在 Discord 頻道設定 → 整合 → Webhook 中建立，複製連結後貼上。點選連結版本時會自動傳送登入連結到該頻道。"
          />
        </div>
      </div>

      <div class="card">
        <div class="row col">
          <span class="row-title">遊戲路徑</span>
        </div>
        <div class="row-sep"></div>
        <div class="path-row">
          <input
            v-model="gamePath"
            class="path-input"
            placeholder="GGM 找不到遊戲時才需設定…"
            spellcheck="false"
          />
          <button class="btn-browse" @click="browse">瀏覽</button>
        </div>
      </div>
    </div>

    <div class="bottom-bar">
      <button class="btn-save" :class="{ done: saved }" @click="save">
        {{ saved ? "已儲存 ✓" : "儲存" }}
      </button>
      <button class="btn-heart" @click="supportAuthor" title="請作者喝杯咖啡">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
          <path d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z"/>
        </svg>
      </button>
      <button class="btn-info" @click="showAbout = true" title="關於">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none">
          <circle cx="12" cy="12" r="9.25" stroke="currentColor" stroke-width="1.7"/>
          <path d="M12 11v5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
          <circle cx="12" cy="7.75" r="1.05" fill="currentColor"/>
        </svg>
      </button>
    </div>

    <div v-if="showAbout" class="about-overlay" @click.self="showAbout = false">
      <div class="about-window">
        <div class="about-titlebar">
          <span class="about-title">關於</span>
          <button class="about-close" @click="showAbout = false" title="關閉">&#x2715;</button>
        </div>
        <div class="about-body">
          <div class="about-card">
            <span class="about-card-label">程式版本</span>
            <div class="version-row">
              <span class="about-card-value">{{ appVersion ? `v${appVersion}` : "—" }}</span>
              <button
                class="btn-update"
                :class="{ ready: updateState === 'available' }"
                :disabled="updateState === 'checking' || updateState === 'downloading'"
                @click="onUpdateClick"
              >
                {{ updateBtnText }}
              </button>
            </div>
          </div>
          <div class="about-card">
            <span class="about-card-label">聯繫作者</span>
            <button class="contact-row" @click="contactDiscord">
              <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
                <path d="M20.3 4.9A19.8 19.8 0 0 0 15.4 3.4l-.24.5a18.3 18.3 0 0 1 4.34 1.35 16.4 16.4 0 0 0-5-1.58 18 18 0 0 0-3 0 16.4 16.4 0 0 0-5 1.58 18.3 18.3 0 0 1 4.34-1.35l-.24-.5A19.8 19.8 0 0 0 3.7 4.9C1.2 8.6.5 12.2.85 15.8a19.9 19.9 0 0 0 6.06 3.06l.73-1.13a13 13 0 0 1-2.05-.98l.5-.37a14.2 14.2 0 0 0 12.02 0l.5.37a13 13 0 0 1-2.05.98l.73 1.13a19.9 19.9 0 0 0 6.06-3.06c.42-4.17-.71-7.74-2.71-10.9ZM9.1 13.9c-.97 0-1.77-.9-1.77-2s.78-2 1.77-2 1.79.9 1.77 2c0 1.1-.79 2-1.77 2Zm5.8 0c-.97 0-1.77-.9-1.77-2s.78-2 1.77-2 1.79.9 1.77 2c0 1.1-.78 2-1.77 2Z"/>
              </svg>
              <span class="contact-text">
                <span class="contact-name">Discord</span>
                <span class="contact-sub" :class="{ copied: discordCopied }">
                  {{ discordCopied ? "已複製 ✓" : `${AUTHOR_DISCORD} · 點擊複製帳號` }}
                </span>
              </span>
            </button>
            <button class="contact-row" @click="openGithub">
              <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
                <path d="M12 2C6.48 2 2 6.58 2 12.25c0 4.53 2.87 8.37 6.84 9.73.5.1.68-.22.68-.49v-1.7c-2.78.62-3.37-1.22-3.37-1.22-.46-1.18-1.11-1.5-1.11-1.5-.9-.63.07-.62.07-.62 1 .07 1.53 1.05 1.53 1.05.89 1.57 2.34 1.12 2.91.85.09-.66.35-1.12.63-1.37-2.22-.26-4.56-1.14-4.56-5.06 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.7 0 0 .84-.28 2.75 1.05a9.34 9.34 0 0 1 5 0c1.91-1.33 2.75-1.05 2.75-1.05.55 1.4.2 2.44.1 2.7.64.72 1.03 1.63 1.03 2.75 0 3.93-2.34 4.8-4.57 5.05.36.32.68.94.68 1.9v2.82c0 .27.18.6.69.49A10.02 10.02 0 0 0 22 12.25C22 6.58 17.52 2 12 2Z"/>
              </svg>
              <span class="contact-text">
                <span class="contact-name">GitHub</span>
                <span class="contact-sub">xense999</span>
              </span>
            </button>
          </div>
        </div>
      </div>
    </div>

  </div>
</template>

<style scoped>
.settings-page {
  display: flex;
  flex-direction: column;
  flex: 1;
  overflow: hidden;
}

.scroll-area {
  flex: 1;
  overflow-y: auto;
  padding: 22px 16px 12px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* ── 卡片 ── */
.card {
  background: var(--surface);
  border: var(--card-border);
  border-radius: 12px;
  overflow: hidden;
  box-shadow: var(--card-shadow);
  backdrop-filter: var(--card-filter);
  -webkit-backdrop-filter: var(--card-filter);
}

/* ── 行 ── */
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 15px 16px;
  min-height: 52px;
}
.row.col {
  flex-direction: column;
  align-items: flex-start;
  gap: 3px;
  padding: 15px 16px 12px;
  min-height: unset;
}

.row-title {
  font-size: 14px;
  font-weight: 400;
  color: var(--text);
}
.row-desc {
  font-size: 12px;
  color: var(--text3);
  line-height: 1.4;
}

.row-sep {
  height: 1px;
  background: var(--border2);
  margin: 0 16px;
}

/* ── 路徑輸入 ── */
.path-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px 14px 16px;
}

.path-input {
  flex: 1;
  min-width: 0;
  background: var(--input-bg);
  border: 1px solid var(--input-border);
  border-radius: 8px;
  padding: 9px 11px;
  font-size: 13px;
  color: var(--text2);
  outline: none;
  transition: border-color 0.15s, color 0.15s;
}
.path-input:focus {
  border-color: var(--primary-color);
  color: var(--text);
}
.path-input::placeholder { color: var(--text3); }

.btn-browse {
  flex-shrink: 0;
  background: var(--surface2);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 9px 14px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text2);
  white-space: nowrap;
  transition: background 0.12s, color 0.12s;
}
.btn-browse:hover { background: var(--surface3); color: var(--text); }

/* ── 贊助愛心 ── */
.btn-heart {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  color: var(--text3);
  transition: background 0.15s, color 0.15s, transform 0.1s;
}
.btn-heart:hover {
  background: rgba(255,59,48,0.12);
  border-color: rgba(255,59,48,0.25);
  color: #ff3b30;
}
.btn-heart:active { transform: scale(0.94); }

.btn-info {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  color: var(--text3);
  transition: background 0.15s, color 0.15s, transform 0.1s;
}
.btn-info:hover { background: var(--glass-hover); color: var(--text); }
.btn-info:active { transform: scale(0.94); }

/* ── 關於（視窗內視窗） ── */
.about-overlay {
  position: absolute;
  inset: 0;
  z-index: 20;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.42);
  backdrop-filter: blur(2px);
  -webkit-backdrop-filter: blur(2px);
}

.about-window {
  width: 300px;
  max-width: calc(100% - 32px);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.28);
  overflow: hidden;
}

.about-titlebar {
  display: flex;
  align-items: center;
  height: 38px;
  padding: 0 8px 0 14px;
  border-bottom: 1px solid var(--border2);
}
.about-title {
  flex: 1;
  font-size: 13px;
  font-weight: 600;
  color: var(--text2);
}
.about-close {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: none;
  color: var(--text3);
  border-radius: 6px;
  font-size: 12px;
  transition: background 0.12s, color 0.12s;
}
.about-close:hover { background: rgba(255,69,58,0.15); color: var(--red); }

.about-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
}

.about-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  background: var(--surface2);
  border: 1px solid var(--border);
  border-radius: 10px;
}
.about-card-label {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--text3);
}
.about-card-value {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
}

.version-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}
.btn-update {
  flex-shrink: 0;
  padding: 5px 12px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text2);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 8px;
  white-space: nowrap;
  transition: background 0.12s, color 0.12s, border-color 0.12s;
}
.btn-update:hover:not(:disabled) { background: var(--glass-hover); color: var(--text); }
.btn-update:disabled { opacity: 0.55; }
.btn-update.ready {
  color: #fff;
  background: var(--primary-color);
  border-color: var(--primary-color);
}

.contact-row {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 10px;
  width: 100%;
  padding: 8px;
  background: none;
  border: none;
  border-radius: 8px;
  color: var(--text2);
  text-align: left;
  transition: background 0.12s;
}
.contact-row:hover { background: var(--glass-hover); }
.contact-row svg { flex-shrink: 0; color: var(--text2); }
.contact-text { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
.contact-name { font-size: 13px; font-weight: 500; color: var(--text); }
.contact-sub {
  font-size: 11px;
  color: var(--text3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.contact-sub.copied { color: var(--primary-color); }

/* ── 分段控制器 ── */
.seg {
  display: flex;
  background: var(--surface2);
  border-radius: 8px;
  padding: 3px;
  gap: 1px;
}
.seg button {
  padding: 6px 18px;
  border-radius: 6px;
  border: none;
  background: none;
  font-size: 13px;
  font-weight: 500;
  color: var(--text2);
  transition: background 0.12s, color 0.12s;
}
.seg button.active {
  background: var(--surface3);
  color: var(--text);
}
.seg button:not(.active):hover { color: var(--text); }

/* ── 底部按鈕 ── */
.bottom-bar {
  flex-shrink: 0;
  display: flex;
  gap: 7px;
  padding: 10px 14px 14px;
  border-top: 1px solid var(--border2);
  background: var(--bg);
}

.btn-save {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  padding: 12px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  font-size: 14px;
  font-weight: 500;
  color: var(--text2);
  transition: background 0.15s, color 0.15s;
}
.btn-save:hover { background: var(--surface2); color: var(--text); }
.btn-save.done {
  background: rgba(50,215,75,0.12);
  border-color: rgba(50,215,75,0.20);
  color: var(--green);
}
</style>

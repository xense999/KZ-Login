<script setup lang="ts">
import { ref, onMounted } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { useTheme } from "../composables/useTheme";

const emit = defineEmits<{ back: [] }>();

const { theme, setTheme } = useTheme();

const STORAGE_KEY = "kusei:game_path";
const WEBHOOK_KEY = "kusei:discord_webhook";

const gamePath = ref("");
const webhookUrl = ref("");
const saved = ref(false);

onMounted(() => {
  gamePath.value = localStorage.getItem(STORAGE_KEY) ?? "";
  webhookUrl.value = localStorage.getItem(WEBHOOK_KEY) ?? "";
});

async function browse() {
  const selected = await open({
    multiple: false,
    filters: [{ name: "執行檔 (*.exe)", extensions: ["exe"] }],
  });
  if (typeof selected === "string") gamePath.value = selected;
}

function save() {
  localStorage.setItem(STORAGE_KEY, gamePath.value.trim());
  const wh = webhookUrl.value.trim();
  if (wh) localStorage.setItem(WEBHOOK_KEY, wh);
  else localStorage.removeItem(WEBHOOK_KEY);
  saved.value = true;
  setTimeout(() => emit('back'), 500);
}

function clear() {
  gamePath.value = "";
  localStorage.removeItem(STORAGE_KEY);
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
          <span class="row-title">遊戲路徑</span>
        </div>
        <div class="row-sep"></div>
        <div class="path-row">
          <input
            v-model="gamePath"
            class="path-input"
            placeholder="尚未設定…"
            spellcheck="false"
          />
          <button class="btn-browse" @click="browse">瀏覽</button>
        </div>
      </div>

      <div class="card">
        <div class="row col">
          <span class="row-title">通知設定</span>
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
    </div>

    <div class="bottom-bar">
      <button class="btn-clear" :disabled="!gamePath" @click="clear">清除路徑</button>
      <button class="btn-save" :class="{ done: saved }" @click="save">
        {{ saved ? "已儲存 ✓" : "儲存" }}
      </button>
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

.btn-clear {
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
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.btn-clear:hover:not(:disabled) { background: var(--surface2); color: var(--text); }
.btn-clear:disabled { opacity: 0.35; cursor: default; }

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

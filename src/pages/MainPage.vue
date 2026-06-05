<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { invoke } from "@tauri-apps/api/core";
import { useAccountsStore, type BeanfunAccount } from "../stores/accounts";

const HUES = [210, 150, 270, 35, 0, 190];
function hue(id: string) {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) & 0xffff;
  return HUES[h % HUES.length];
}

defineEmits<{ addAccount: []; reauth: [string] }>();

const store = useAccountsStore();
const expanded = ref<Set<string>>(new Set());

const contextMenu = ref<{ x: number; y: number; accountId: string } | null>(null);
const menuStyle = computed(() => {
  if (!contextMenu.value) return {};
  return {
    left: `${Math.min(contextMenu.value.x, 252)}px`,
    top: `${Math.min(contextMenu.value.y, 544)}px`,
  };
});

function openContextMenu(e: MouseEvent, accountId: string) {
  e.stopPropagation();
  contextMenu.value = { x: e.clientX, y: e.clientY, accountId };
}
function closeContextMenu() { contextMenu.value = null; }

function deleteAccount(accountId: string) {
  store.removeAccount(accountId);
  expanded.value.delete(accountId);
  closeContextMenu();
}

const renamingAlias = ref<string | null>(null);
const aliasValue = ref("");

function startRenameAlias(accountId: string, current: string) {
  renamingAlias.value = accountId;
  aliasValue.value = current;
  closeContextMenu();
  setTimeout(() => {
    const el = document.getElementById(`alias-input-${accountId}`) as HTMLInputElement;
    el?.focus(); el?.select();
  }, 10);
}
function commitRenameAlias(accountId: string) {
  const alias = aliasValue.value.trim() || "Beanfun 帳號";
  store.updateAlias(accountId, alias);

  // Save alias memory keyed by sorted sub-account names
  const acc = store.accounts.find(a => a.id === accountId);
  if (acc && alias !== "Beanfun 帳號") {
    const key = acc.gameAccounts.map(g => g.sid).sort().join("|");
    const mem: Record<string, string> = JSON.parse(
      localStorage.getItem("kusei:alias_memory") ?? "{}"
    );
    mem[key] = alias;
    localStorage.setItem("kusei:alias_memory", JSON.stringify(mem));
  }

  renamingAlias.value = null;
}

const gamePath = ref("");
onMounted(() => {
  gamePath.value = localStorage.getItem("kusei:game_path") ?? "";
});

async function launchGame() {
  if (!gamePath.value) return;
  try {
    await invoke("launch_game", { path: gamePath.value });
  } catch (e) {
    alert(String(e));
  }
}

// Account-level drag (whole row, threshold-based)
const draggingAcc = ref<number | null>(null);
const dragTargetAcc = ref<number | null>(null);
const accDragState = ref<{ fromIdx: number; startX: number; startY: number; isDragging: boolean } | null>(null);
const suppressAccClick = ref<string | null>(null);

function onAccRowPointerDown(e: PointerEvent, idx: number) {
  if ((e.target as HTMLElement).closest('button, input')) return;
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  accDragState.value = { fromIdx: idx, startX: e.clientX, startY: e.clientY, isDragging: false };
  dragTargetAcc.value = idx;
}
function onAccRowPointerMove(e: PointerEvent) {
  if (!accDragState.value) return;
  const dist = Math.hypot(e.clientX - accDragState.value.startX, e.clientY - accDragState.value.startY);
  if (!accDragState.value.isDragging && dist > 8) {
    accDragState.value.isDragging = true;
    draggingAcc.value = accDragState.value.fromIdx;
  }
  if (accDragState.value.isDragging) {
    const el = document.elementFromPoint(e.clientX, e.clientY);
    const card = el?.closest('[data-acc-idx]') as HTMLElement | null;
    if (card?.dataset.accIdx !== undefined) dragTargetAcc.value = parseInt(card.dataset.accIdx);
  }
}
function onAccRowPointerUp(e: PointerEvent, accId: string) {
  if (!accDragState.value) return;
  if (accDragState.value.isDragging) {
    if (dragTargetAcc.value !== null && dragTargetAcc.value !== accDragState.value.fromIdx)
      store.moveAccount(accDragState.value.fromIdx, dragTargetAcc.value);
    draggingAcc.value = null;
    dragTargetAcc.value = null;
    suppressAccClick.value = accId;
    setTimeout(() => { suppressAccClick.value = null; }, 50);
  }
  (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  accDragState.value = null;
}
function onAccRowClick(accId: string) {
  if (suppressAccClick.value === accId) return;
  toggleAccount(accId);
}

// Game-level drag
const dragging = ref<{ accountId: string; fromIdx: number } | null>(null);
const dragTarget = ref<number | null>(null);

function onPointerDown(e: PointerEvent, accountId: string, idx: number) {
  e.preventDefault();
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  dragging.value = { accountId, fromIdx: idx };
  dragTarget.value = idx;
}

function onPointerMove(e: PointerEvent, accountId: string) {
  if (!dragging.value || dragging.value.accountId !== accountId) return;
  const el = document.elementFromPoint(e.clientX, e.clientY);
  const item = el?.closest('[data-game-idx]') as HTMLElement | null;
  if (item?.dataset.gameIdx !== undefined) {
    dragTarget.value = parseInt(item.dataset.gameIdx);
  }
}

function onPointerUp(e: PointerEvent, accountId: string) {
  if (dragging.value?.accountId === accountId && dragTarget.value !== null
      && dragTarget.value !== dragging.value.fromIdx) {
    store.moveGameAccount(accountId, dragging.value.fromIdx, dragTarget.value);
  }
  (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  dragging.value = null;
  dragTarget.value = null;
}

function cancelDrag() {
  dragging.value = null;
  dragTarget.value = null;
}

const renaming = ref<{ accountId: string; sn: string } | null>(null);
const renameValue = ref("");
const copiedAccount = ref<Set<string>>(new Set());
const copiedOtp = ref<Set<string>>(new Set());
const loadingOtp = ref<Set<string>>(new Set());
const errorMap = ref<Record<string, string>>({});

function toggleAccount(id: string) {
  if (expanded.value.has(id)) expanded.value.delete(id);
  else expanded.value.add(id);
}

function startRename(accountId: string, sn: string, current: string) {
  renaming.value = { accountId, sn };
  renameValue.value = current;
  setTimeout(() => {
    const el = document.getElementById(`rename-${sn}`) as HTMLInputElement;
    el?.focus(); el?.select();
  }, 10);
}

function commitRename(accountId: string, sn: string) {
  const trimmed = renameValue.value.trim();
  store.updateGameName(accountId, sn, trimmed);
  renaming.value = null;

  const acc = store.accounts.find(a => a.id === accountId);
  const game = acc?.gameAccounts.find(g => g.sn === sn);
  if (game) {
    const mem: Record<string, string> = JSON.parse(
      localStorage.getItem("kusei:name_memory") ?? "{}"
    );
    if (trimmed) {
      mem[game.sid] = trimmed;
    } else {
      delete mem[game.sid];
    }
    localStorage.setItem("kusei:name_memory", JSON.stringify(mem));
  }
}

async function copyAccountId(sid: string, sn: string) {
  await writeText(sid);
  copiedAccount.value.add(sn);
  setTimeout(() => copiedAccount.value.delete(sn), 1800);
}

async function copyOtp(account: BeanfunAccount, sn: string) {
  if (!account.token) { errorMap.value[sn] = "SESSION_EXPIRED"; return; }
  loadingOtp.value.add(sn);
  errorMap.value[sn] = "";
  try {
    const result = await store.fetchOtp(account.id, sn);
    await writeText(result.otp);
    copiedOtp.value.add(sn);
    setTimeout(() => copiedOtp.value.delete(sn), 1800);
  } catch (e: unknown) {
    const raw = e instanceof Error ? e.message : String(e);
    const msg = cleanError(raw);
    if (msg === "SESSION_EXPIRED") {
      store.invalidateToken(account.id);
      errorMap.value[sn] = "SESSION_EXPIRED";
    } else {
      errorMap.value[sn] = msg;
    }
  } finally {
    loadingOtp.value.delete(sn);
  }
}

function displayName(game: { sname: string; localName: string | null }) {
  return game.localName || game.sname;
}

const autoLogging = ref<Set<string>>(new Set());
const sentMap = ref<Record<string, { sid: string }>>({});

async function autoLogin(account: BeanfunAccount, game: { sn: string; sid: string; sname: string }) {
  if (!account.token) return;
  autoLogging.value.add(game.sn);
  errorMap.value[game.sn] = "";
  delete sentMap.value[game.sn];
  try {
    const result = await invoke<{ sid: string; otp: string }>('auto_login', {
      token: account.token,
      accountSn: game.sn,
      accountSid: game.sid,
      accountSname: game.sname,
    });
    sentMap.value[game.sn] = { sid: result.sid };
    setTimeout(() => delete sentMap.value[game.sn], 4000);
  } catch (e: unknown) {
    const raw = e instanceof Error ? e.message : String(e);
    errorMap.value[game.sn] = cleanError(raw);
  } finally {
    autoLogging.value.delete(game.sn);
  }
}


function cleanError(msg: string): string {
  if (msg.includes("SESSION_EXPIRED")) return "SESSION_EXPIRED";
  if (msg.length > 100 || msg.includes("<!DOCTYPE") || msg.includes("<html") || msg.includes("long polling")) {
    if (msg.includes("尚未登入") || msg.includes("SESSION_EXPIRED") || msg.includes("long polling"))
      return "SESSION_EXPIRED";
    return "伺服器回應異常，請稍後再試";
  }
  return msg;
}
</script>

<template>
  <div class="page-layout">
  <div class="scroll">
    <div v-if="store.accounts.length === 0" class="empty">
      <div class="empty-icon">
        <svg viewBox="0 0 40 40" fill="none" xmlns="http://www.w3.org/2000/svg" width="40" height="40">
          <circle cx="20" cy="14" r="7" stroke="currentColor" stroke-width="1.5"/>
          <path d="M6 36c0-7.732 6.268-14 14-14s14 6.268 14 14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
      </div>
      <div class="empty-txt">尚無帳號</div>
      <div class="empty-sub">點下方新增 Beanfun 帳號</div>
    </div>

    <div
      v-for="(acc, accIdx) in store.accounts" :key="acc.id"
      class="card"
      :data-acc-idx="accIdx"
      :class="{
        'acc-drag-over': draggingAcc !== null && dragTargetAcc === accIdx && draggingAcc !== accIdx,
        'acc-is-dragging': draggingAcc === accIdx
      }"
    >
      <div class="acc-row"
        @pointerdown="onAccRowPointerDown($event, accIdx)"
        @pointermove="onAccRowPointerMove"
        @pointerup="onAccRowPointerUp($event, acc.id)"
        @pointercancel="accDragState = null; draggingAcc = null; dragTargetAcc = null"
        @click="onAccRowClick(acc.id)"
        @contextmenu.prevent="openContextMenu($event, acc.id)"
      >
        <div class="av" :style="{ background: `hsl(${hue(acc.id)},40%,18%)`, color: `hsl(${hue(acc.id)},70%,70%)` }">
          <img src="/avatar.png" class="av-icon" />
        </div>
        <div class="acc-info">
          <template v-if="renamingAlias === acc.id">
            <input :id="`alias-input-${acc.id}`" v-model="aliasValue" class="alias-input"
              @keydown.enter="commitRenameAlias(acc.id)"
              @keydown.escape="renamingAlias = null"
              @blur="commitRenameAlias(acc.id)"
              @click.stop />
          </template>
          <template v-else>
            <span class="acc-name">{{ acc.alias }}</span>
            <button class="alias-rename-btn" @click.stop="startRenameAlias(acc.id, acc.alias)" title="重新命名">
              <svg viewBox="0 0 16 16" fill="none" width="11" height="11">
                <path d="M11.5 2.5a1.414 1.414 0 0 1 2 2L5 13H3v-2L11.5 2.5z"
                  stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>
              </svg>
            </button>
          </template>
        </div>
        <div class="acc-right">
<div v-if="acc.token" class="dot on" title="已連線"></div>
          <template v-else>
            <div class="dot off" title="已斷線，請重新登入"></div>
            <button class="qr-rescan" @click.stop="$emit('reauth', acc.id)" title="重新登入">
              <svg viewBox="0 0 16 16" fill="none" width="15" height="15">
                <path d="M5 1.5H2.5a1 1 0 0 0-1 1V5M11 1.5h2.5a1 1 0 0 1 1 1V5M5 14.5H2.5a1 1 0 0 1-1-1V11M11 14.5h2.5a1 1 0 0 0 1-1V11"
                  stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
                <rect x="4.5" y="4.5" width="7" height="7" rx="1" stroke="currentColor" stroke-width="1.3"/>
                <rect x="6.5" y="6.5" width="3" height="3" rx="0.5" fill="currentColor"/>
              </svg>
            </button>
          </template>
          <svg class="chev" :class="{ open: expanded.has(acc.id) }" viewBox="0 0 16 16" fill="none" width="14" height="14">
            <path d="M4 6l4 4 4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
        </div>
      </div>

      <div v-if="expanded.has(acc.id)" class="game-list">
        <div
          v-for="(game, idx) in acc.gameAccounts" :key="game.sn"
          class="game-item"
          :data-game-idx="idx"
          :class="{
            'drag-over': dragging?.accountId === acc.id && dragTarget === idx && dragging.fromIdx !== idx,
            'is-dragging': dragging?.accountId === acc.id && dragging.fromIdx === idx
          }"
        >
          <div class="game-row">
            <div
              class="drag-handle"
              title="拖移排序"
              @pointerdown="onPointerDown($event, acc.id, idx)"
              @pointermove="onPointerMove($event, acc.id)"
              @pointerup="onPointerUp($event, acc.id)"
              @pointercancel="cancelDrag"
            >
              <svg viewBox="0 0 10 16" fill="none" width="10" height="16">
                <circle cx="3" cy="4" r="1.2" fill="currentColor"/>
                <circle cx="7" cy="4" r="1.2" fill="currentColor"/>
                <circle cx="3" cy="8" r="1.2" fill="currentColor"/>
                <circle cx="7" cy="8" r="1.2" fill="currentColor"/>
                <circle cx="3" cy="12" r="1.2" fill="currentColor"/>
                <circle cx="7" cy="12" r="1.2" fill="currentColor"/>
              </svg>
            </div>
            <div class="game-name-wrap">
              <template v-if="renaming?.accountId === acc.id && renaming?.sn === game.sn">
                <input :id="`rename-${game.sn}`" v-model="renameValue" class="rename-input"
                  :placeholder="displayName(game)"
                  @keydown.enter="commitRename(acc.id, game.sn)"
                  @keydown.escape="renaming = null"
                  @blur="commitRename(acc.id, game.sn)" />
              </template>
              <template v-else>
                <span class="game-nm">{{ displayName(game) }}</span>
                <button class="rename-btn" @click.stop="startRename(acc.id, game.sn, displayName(game))" title="改名">
                  <svg viewBox="0 0 16 16" fill="none" width="11" height="11">
                    <path d="M11.5 2.5a1.414 1.414 0 0 1 2 2L5 13H3v-2L11.5 2.5z"
                      stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>
                  </svg>
                </button>
              </template>
            </div>

            <div class="btns">
              <button class="btn-pill" :class="{ done: copiedAccount.has(game.sn) }"
                title="複製帳號 ID"
                @click.stop="copyAccountId(game.sid, game.sn)">
                {{ copiedAccount.has(game.sn) ? "✓" : "帳號" }}
              </button>
              <button class="btn-pill primary" :class="{ done: copiedOtp.has(game.sn) }"
                :disabled="!acc.token || loadingOtp.has(game.sn)"
                title="複製密碼 (OTP)"
                @click.stop="copyOtp(acc, game.sn)">
                <span v-if="loadingOtp.has(game.sn)" class="spin"></span>
                <template v-else>{{ copiedOtp.has(game.sn) ? "✓" : "密碼" }}</template>
              </button>
              <button class="btn-pill auto-btn"
                :disabled="!acc.token || autoLogging.has(game.sn) || loadingOtp.has(game.sn)"
                @click.stop="autoLogin(acc, game)"
                title="自動登入遊戲">
                <span v-if="autoLogging.has(game.sn)" class="spin"></span>
                <svg v-else viewBox="0 0 14 14" fill="none" width="12" height="12">
                  <path d="M6 2.5L11.5 7 6 11.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                  <path d="M2 7h9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                </svg>
              </button>
            </div>
          </div>

          <div v-if="errorMap[game.sn] === 'SESSION_EXPIRED'" class="err-row">登入已失效，請重新掃描</div>
          <div v-else-if="errorMap[game.sn]" class="err-row">{{ errorMap[game.sn] }}</div>
          <div v-else-if="sentMap[game.sn]" class="sent-row">
            已填入帳號：{{ sentMap[game.sn].sid }}
          </div>
        </div>
      </div>
    </div>

    <template v-if="contextMenu">
      <div class="ctx-overlay" @click="closeContextMenu" @contextmenu.prevent="closeContextMenu"></div>
      <div class="ctx-menu" :style="menuStyle">
        <button class="ctx-item" @click="startRenameAlias(contextMenu.accountId, store.accounts.find(a => a.id === contextMenu!.accountId)?.alias ?? '')">
          重新命名
        </button>
        <div class="ctx-sep"></div>
        <button class="ctx-item danger" @click="deleteAccount(contextMenu.accountId)">
          刪除帳號
        </button>
      </div>
    </template>
  </div>

  <div class="bottom-bar">
    <button class="btn-add" @click="$emit('addAccount')">
      <svg viewBox="0 0 16 16" fill="none" width="14" height="14">
        <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
      </svg>
      新增帳號
    </button>
    <button class="btn-launch" :disabled="!gamePath" @click="launchGame">
      <svg viewBox="0 0 16 16" fill="none" width="14" height="14">
        <path d="M4 3l10 5-10 5V3z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" fill="currentColor" fill-opacity="0.18"/>
      </svg>
      啟動遊戲
    </button>
  </div>

  </div>
</template>

<style scoped>
.page-layout {
  display: flex;
  flex-direction: column;
  flex: 1;
  overflow: hidden;
}

.scroll {
  padding: 16px 14px 8px;
  display: flex;
  flex-direction: column;
  gap: 7px;
  overflow-y: auto;
  flex: 1;
}

.bottom-bar {
  flex-shrink: 0;
  display: flex;
  gap: 7px;
  padding: 10px 14px 14px;
  border-top: 1px solid var(--border2);
  background: var(--bg);
}

/* Empty */
.empty {
  display: flex; flex-direction: column; align-items: center;
  gap: 8px; padding: 36px 0 28px; color: var(--text3);
}
.empty-icon { opacity: 0.4; }
.empty-txt { font-size: 14px; font-weight: 500; color: var(--text2); }
.empty-sub { font-size: 12px; }

/* Card */
.card {
  background: var(--surface);
  border: var(--card-border);
  border-radius: 12px;
  overflow: hidden;
  flex-shrink: 0;
  box-shadow: var(--card-shadow);
  backdrop-filter: var(--card-filter);
  -webkit-backdrop-filter: var(--card-filter);
}

.card.acc-drag-over { box-shadow: inset 0 0 0 1px var(--border), var(--card-shadow); background: var(--hover-tint); }
.card.acc-is-dragging { opacity: 0.4; }

.acc-row {
  display: flex; align-items: center; gap: 10px;
  padding: 11px 13px; cursor: grab;
  touch-action: none;
  transition: background 0.15s;
}
.acc-row:hover { background: var(--hover-tint); }
.acc-row:hover .alias-rename-btn { opacity: 1; }
.acc-is-dragging .acc-row { cursor: grabbing; }

.alias-rename-btn {
  background: none; border: none; padding: 3px;
  color: var(--text3); border-radius: 4px;
  opacity: 0; transition: opacity 0.12s, background 0.12s;
  display: flex; align-items: center; flex-shrink: 0;
  margin-left: 2px;
}
.alias-rename-btn:hover { background: var(--glass-hover); color: var(--text2); opacity: 1; }

.av {
  width: 34px; height: 34px; border-radius: 50%; flex-shrink: 0;
  display: flex; align-items: center; justify-content: center;
  font-size: 13px; font-weight: 600;
  overflow: hidden;
}
.av-icon {
  width: 100%; height: 100%; object-fit: cover;
}

.acc-info { flex: 1; min-width: 0; display: flex; align-items: center; gap: 5px; }
.acc-name {
  font-size: 14px; font-weight: 500; color: var(--text);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}

.alias-input {
  background: var(--input-bg); border: 1px solid var(--input-border);
  border-radius: 6px; padding: 3px 8px; font-size: 14px; font-weight: 500;
  color: var(--text); outline: none; width: 140px;
}

.acc-right { display: flex; align-items: center; gap: 7px; }

.dot { width: 6px; height: 6px; border-radius: 50%; flex-shrink: 0; }
.dot.on { background: var(--green); box-shadow: 0 0 6px rgba(50,215,75,0.45); }
.dot.off { background: var(--red); box-shadow: 0 0 6px rgba(255,69,58,0.5); animation: blink 1.4s ease-in-out infinite; }
@keyframes blink { 0%,100% { opacity: 1; } 50% { opacity: 0.35; } }

.qr-rescan {
  background: none; border: none; padding: 2px;
  color: var(--warn); border-radius: 4px;
  display: flex; align-items: center;
  transition: background 0.12s, color 0.12s;
}
.qr-rescan:hover { background: rgba(255,159,10,0.12); }

.chev { color: var(--text3); transition: transform 0.2s ease; }
.chev.open { transform: rotate(180deg); }

/* Game list */
.game-list { border-top: 1px solid var(--border2); }
.game-item { border-bottom: 1px solid var(--border2); }
.game-item:last-child { border-bottom: none; }

.game-item.drag-over { background: var(--hover-tint); box-shadow: inset 0 0 0 1px var(--border); }
.game-item.is-dragging { opacity: 0.4; }

.game-row {
  display: flex; align-items: center; justify-content: space-between;
  padding: 9px 13px; gap: 8px;
  transition: background 0.12s;
}
.game-row:hover { background: var(--hover-tint); }
.game-row:hover .rename-btn { opacity: 1; }
.game-row:hover .drag-handle { opacity: 1; }

.drag-handle {
  flex-shrink: 0;
  color: var(--text3);
  opacity: 0.25;
  cursor: grab;
  display: flex;
  align-items: center;
  padding: 2px 4px 2px 0;
  touch-action: none;
  transition: opacity 0.12s;
}
.game-row:hover .drag-handle { opacity: 0.6; }
.drag-handle:hover { opacity: 1 !important; }
.drag-handle:active { cursor: grabbing; opacity: 1 !important; }

.game-name-wrap {
  display: flex; align-items: center; gap: 5px; flex: 1; min-width: 0;
}
.game-nm {
  font-size: 13px; font-weight: 500; color: var(--text);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.rename-btn {
  background: none; border: none; padding: 3px;
  color: var(--text3); border-radius: 4px;
  opacity: 0; transition: opacity 0.12s, background 0.12s;
  display: flex; align-items: center; flex-shrink: 0;
}
.rename-btn:hover { background: var(--glass-hover); color: var(--text2); opacity: 1; }

.rename-input {
  background: var(--input-bg); border: 1px solid var(--input-border);
  border-radius: 6px; padding: 4px 8px; font-size: 13px;
  color: var(--text); outline: none; width: 120px;
}

.btns { display: flex; gap: 5px; flex-shrink: 0; }

.btn-pill {
  border: 1px solid var(--border);
  background: var(--glass);
  border-radius: 20px;
  padding: 5px 14px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text2);
  transition: background 0.15s, color 0.15s, border-color 0.15s;
  min-width: 52px;
  text-align: center;
}
.btn-pill:hover { background: var(--glass-hover); color: var(--text); border-color: var(--pill-hover-border); }
.btn-pill.primary { color: var(--text2); border-color: var(--border); background: var(--glass); }
.btn-pill.primary:hover { background: var(--glass-hover); color: var(--text); border-color: var(--pill-hover-border); }
.btn-pill:disabled { opacity: 0.3; cursor: default; }
.btn-pill.done { color: var(--green); border-color: rgba(52,199,89,0.3); background: rgba(52,199,89,0.08); }
.btn-pill.auto-btn { min-width: 32px; padding: 5px 8px; }

.spin {
  display: inline-block; width: 11px; height: 11px;
  border: 1.5px solid var(--spin-track);
  border-top-color: var(--text);
  border-radius: 50%; animation: spin 0.7s linear infinite;
  vertical-align: middle;
}
@keyframes spin { to { transform: rotate(360deg); } }

.err-row  { font-size: 11px; color: var(--warn);  padding: 5px 13px 7px 20px; }
.sent-row { font-size: 11px; color: var(--text3); padding: 5px 13px 7px 20px; font-variant-numeric: tabular-nums; }

/* Context menu */
.ctx-overlay {
  position: fixed; inset: 0; z-index: 90;
}

.ctx-menu {
  position: fixed; z-index: 91;
  background: var(--ctx-menu-bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 4px;
  min-width: 148px;
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  box-shadow: var(--ctx-shadow);
}

.ctx-item {
  display: flex; align-items: center; justify-content: flex-start;
  width: 100%;
  padding: 7px 10px; font-size: 13px; font-weight: 400;
  background: none; border: none; border-radius: 6px;
  color: var(--text); transition: background 0.12s;
}
.ctx-item:hover { background: var(--ctx-hover); }
.ctx-item.danger { color: var(--red); }
.ctx-item.danger:hover { background: rgba(255,69,58,0.12); }

.ctx-sep {
  height: 1px; background: var(--ctx-sep-color); margin: 3px 6px;
}

/* Bottom bar buttons */
.btn-add {
  flex: 1;
  display: flex; align-items: center; justify-content: center;
  gap: 7px; padding: 12px;
  background: var(--surface); border: 1px solid var(--border);
  border-radius: 12px; font-size: 14px; font-weight: 500;
  color: var(--text2); transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.btn-add:hover { background: var(--surface2); color: var(--text); }

.btn-launch {
  flex: 1;
  display: flex; align-items: center; justify-content: center;
  gap: 6px; padding: 12px;
  background: var(--surface); border: 1px solid var(--border);
  border-radius: 12px; font-size: 14px; font-weight: 500;
  color: var(--text2); transition: background 0.15s, color 0.15s;
}
.btn-launch:hover:not(:disabled) { background: var(--surface2); color: var(--text); }
.btn-launch:disabled { opacity: 0.3; cursor: default; }
</style>

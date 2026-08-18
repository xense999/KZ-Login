<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { Window } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// 別名與版面幾何都由 Rust 開窗時放進 query string——殼層拿不到主視窗的 store，
// 而幾何的單一來源在 Rust（它同時用這些值算分頁視窗的位置）。
const params = new URLSearchParams(location.search);
const alias = params.get("alias") ?? "帳號瀏覽器";
const titlebarH = params.get("titlebar") ?? "42";
const navbarH = params.get("navbar") ?? "42";
const edge = params.get("edge") ?? "3";

type Direction =
  | "North" | "South" | "East" | "West"
  | "NorthEast" | "NorthWest" | "SouthEast" | "SouthWest";

type NavState = { url: string };
type TabEntry = { id: number; title: string; active: boolean };

const tabs = ref<TabEntry[]>([]);
const url = ref("");
// 使用者正在打字時不要被頁面推來的網址蓋掉
const editing = ref(false);
const draft = ref("");

let unlistenNav: UnlistenFn | null = null;
let unlistenTabs: UnlistenFn | null = null;
let activeId: number | null = null;

onMounted(async () => {
  unlistenNav = await listen<NavState>("browser://nav", (e) => {
    url.value = e.payload.url;
    if (!editing.value) draft.value = e.payload.url;
  });
  unlistenTabs = await listen<TabEntry[]>("browser://tabs", (e) => {
    tabs.value = e.payload;
    // 換了作用中分頁＝放棄未送出的編輯，網址列一定顯示該分頁的網址。
    // 沒這段的話，點過網址列（editing 還開著）再切分頁，網址列會停在舊分頁。
    // Rust 端先推 nav 再推 tabs，所以這時 url 已經是新分頁的網址。
    const nowActive = e.payload.find((t) => t.active)?.id ?? null;
    if (nowActive !== activeId) {
      activeId = nowActive;
      editing.value = false;
      draft.value = url.value;
      navError.value = "";
    }
  });
});

onUnmounted(() => {
  unlistenNav?.();
  unlistenTabs?.();
});

function minimize() { Window.getCurrent().minimize(); }
function close() { Window.getCurrent().close(); }

// 導覽與分頁的錯誤一律顯示出來。吞掉的話「按了沒反應」就查不出是哪一層壞掉。
const navError = ref("");

async function nav(action: "back" | "forward" | "reload") {
  navError.value = "";
  try {
    await invoke("browser_navigate", { action });
  } catch (e) {
    navError.value = e instanceof Error ? e.message : String(e);
  }
}

async function go() {
  navError.value = "";
  try {
    await invoke("browser_navigate", { action: "goto", url: draft.value });
    editing.value = false;
  } catch (e) {
    navError.value = e instanceof Error ? e.message : String(e);
  }
}

async function tabAction(action: "new" | "activate" | "close", id?: number) {
  navError.value = "";
  try {
    await invoke("browser_tab", { action, id });
  } catch (e) {
    navError.value = e instanceof Error ? e.message : String(e);
  }
}

// 只有 Escape 放棄編輯。不掛在 blur 上：焦點被分頁視窗搶走時會把使用者
// 打好的網址一起清掉，看起來就像「網址列不能輸入」。
function cancelEdit() {
  editing.value = false;
  draft.value = url.value;
  navError.value = "";
}

// Windows 上 decorations:false 的視窗沒有原生 resize 邊框，分頁視窗又蓋住框內
// 整塊，縮放只能自己接。握把都放在 .frame 裡面，靠它的 overflow:hidden
// ＋圓角一起裁切，握把才不會凸出圓角外面去。
function startResize(e: MouseEvent, direction: Direction) {
  if (e.button !== 0) return;
  e.preventDefault();
  Window.getCurrent().startResizeDragging(direction);
}
</script>

<template>
  <!-- 鋪滿整個視窗：視窗大小就是可視範圍。圓角只做上面兩角——下面兩角是方的
       （使用者定的外型），分頁視窗的直角也就壓不到圓弧。 -->
  <div class="frame" :style="{
    '--titlebar-h': `${titlebarH}px`,
    '--navbar-h': `${navbarH}px`,
    '--frame-edge': `${edge}px`,
  }">
    <div class="titlebar shell-titlebar" data-tauri-drag-region>
      <span class="alias" data-tauri-drag-region>{{ alias }}</span>
      <span class="divider">|</span>

      <!-- 分頁：每個分頁本身是一個框。「|」只隔開帳號名稱與分頁區。 -->
      <div class="tabstrip">
        <button
          v-for="t in tabs"
          :key="t.id"
          class="tab"
          :class="{ active: t.active }"
          :title="t.title"
          @click="tabAction('activate', t.id)"
          @mousedown.middle.prevent="tabAction('close', t.id)"
        >
          <span class="tab-title">{{ t.title }}</span>
          <span class="tab-close" title="關閉分頁" @click.stop="tabAction('close', t.id)">
            &#x2715;
          </span>
        </button>
        <button class="tab-new" title="新分頁" @click="tabAction('new')">＋</button>
      </div>

      <span class="titlebar-space" data-tauri-drag-region></span>
      <button class="wbtn" title="縮小" @click="minimize">&#x2212;</button>
      <button class="wbtn close" title="關閉" @click="close">&#x2715;</button>
    </div>

    <div class="navbar">
      <!-- 上下頁鈕一律可按：問 WebView2「能不能上一頁」得在事件回呼裡阻塞，
           那會把主執行緒卡死。沒有歷史時按了沒事。 -->
      <button class="nbtn" title="上一頁" @click="nav('back')">
        <svg viewBox="0 0 16 16" fill="none" width="15" height="15">
          <path d="M10 3.5L5.5 8l4.5 4.5" stroke="currentColor" stroke-width="1.5"
            stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>
      <button class="nbtn" title="下一頁" @click="nav('forward')">
        <svg viewBox="0 0 16 16" fill="none" width="15" height="15">
          <path d="M6 3.5L10.5 8 6 12.5" stroke="currentColor" stroke-width="1.5"
            stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>
      <button class="nbtn" title="重新整理" @click="nav('reload')">
        <svg viewBox="0 0 16 16" fill="none" width="15" height="15">
          <path d="M13 8a5 5 0 1 1-1.6-3.66" stroke="currentColor" stroke-width="1.5"
            stroke-linecap="round"/>
          <path d="M13 2.5v2.8h-2.8" stroke="currentColor" stroke-width="1.5"
            stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>

      <!-- 錯誤訊息蓋在網址列上，不動版面：工具列高度是 Rust 算好的，這裡不能長高，
           而網址列下方那塊被分頁視窗蓋著、放什麼都看不到。 -->
      <div class="url-wrap">
        <input
          class="urlbar"
          :class="{ bad: !!navError }"
          v-model="draft"
          :title="url"
          spellcheck="false"
          placeholder="輸入網址或搜尋字詞後按 Enter"
          @focus="editing = true"
          @keydown.enter="go"
          @keydown.escape="cancelEdit"
        />
        <div v-if="navError" class="nav-error" :title="navError" @click="navError = ''">
          {{ navError }}
        </div>
      </div>
      <button class="nbtn" title="前往" @click="go">
        <svg viewBox="0 0 16 16" fill="none" width="15" height="15">
          <path d="M3 8h9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          <path d="M8.5 4.5L12 8l-3.5 3.5" stroke="currentColor" stroke-width="1.5"
            stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>
    </div>

    <!-- 縮放握把。角落排在邊之後才會蓋在邊上面（同 z 層後者優先）。 -->
    <div class="rh north" @mousedown="startResize($event, 'North')"></div>
    <div class="rh south" @mousedown="startResize($event, 'South')"></div>
    <div class="rh west"  @mousedown="startResize($event, 'West')"></div>
    <div class="rh east"  @mousedown="startResize($event, 'East')"></div>
    <div class="rh nw" @mousedown="startResize($event, 'NorthWest')"></div>
    <div class="rh ne" @mousedown="startResize($event, 'NorthEast')"></div>
    <div class="rh sw" @mousedown="startResize($event, 'SouthWest')"></div>
    <div class="rh se" @mousedown="startResize($event, 'SouthEast')"></div>
  </div>
</template>

<style scoped>
/* 邊框用 inset box-shadow 畫、不用 border：border 會把子元素的定位原點推進來
   一圈，握把就蓋不到邊框那幾 px（也就是唯一抓得到的地方）。 */
.frame {
  position: relative;
  width: 100%;
  height: 100%;
  background: var(--bg);
  box-shadow: inset 0 0 0 var(--frame-edge) var(--edge);
  /* 上兩角圓、下兩角方（使用者定的外型）。視窗是 transparent，圓弧外是桌面。
     打字卡死與透明無關（2026-08-18 已坐實是 multi-webview 的問題）。 */
  border-radius: 10px 10px 0 0;
  overflow: hidden;
}

.shell-titlebar {
  box-sizing: border-box;
  height: var(--titlebar-h);
  padding: 0 8px 0 14px;
  gap: 6px;
}

.alias {
  flex-shrink: 0;
  max-width: 160px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text2);
  letter-spacing: 0.01em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.divider {
  flex-shrink: 0;
  color: var(--text3);
  opacity: 0.6;
  font-size: 13px;
  pointer-events: none;
}

.tabstrip {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  overflow: hidden;
}

/* 每個分頁自己是一個框 */
.tab {
  display: flex;
  align-items: center;
  gap: 5px;
  flex: 0 1 160px;
  min-width: 56px;
  height: 26px;
  padding: 0 6px 0 9px;
  background: var(--input-bg);
  border: 1px solid var(--input-border);
  border-radius: 7px;
  color: var(--text3);
  font-size: 12px;
  transition: background 0.12s, color 0.12s, border-color 0.12s;
}
.tab:hover { color: var(--text2); background: var(--glass-hover); }
.tab.active {
  color: var(--text);
  border-color: var(--primary-border);
  background: var(--glass-hover);
}

.tab-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: left;
}

.tab-close {
  flex-shrink: 0;
  width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  font-size: 9px;
  color: var(--text3);
}
.tab-close:hover { background: rgba(255,69,58,0.15); color: var(--red); }

.tab-new {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  background: none;
  border: none;
  color: var(--text3);
  font-size: 13px;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.12s, color 0.12s;
}
.tab-new:hover { background: var(--glass-hover); color: var(--text2); }

.titlebar-space { flex: 1; min-width: 8px; }

.navbar {
  box-sizing: border-box;
  height: var(--navbar-h);
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 10px 0 8px;
  border-bottom: 1px solid var(--border2);
}

.nbtn {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  background: none;
  border: none;
  color: var(--text3);
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.12s, color 0.12s;
}
.nbtn:hover:not(:disabled) { background: var(--glass-hover); color: var(--text2); }
.nbtn:disabled { opacity: 0.3; cursor: default; }

.url-wrap {
  position: relative;
  flex: 1;
  min-width: 0;
  margin-left: 4px;
}

.urlbar {
  width: 100%;
  height: 26px;
  padding: 0 10px;
  background: var(--input-bg);
  border: 1px solid var(--input-border);
  border-radius: 8px;
  font-size: 12px;
  color: var(--text2);
  outline: none;
  user-select: text;
  -webkit-user-select: text;
}
.urlbar:focus { border-color: var(--primary-border); color: var(--text); }
.urlbar.bad { border-color: var(--red); }

.nav-error {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  padding: 0 10px;
  border-radius: 8px;
  background: var(--surface);
  border: 1px solid var(--red);
  color: var(--red);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}

.rh { position: absolute; }
/* 上邊可以比邊框寬——它落在標題列上，不是壓在分頁視窗上，多給不佔空間 */
.north { top: 0; left: 0; right: 0; height: calc(var(--frame-edge) + 3px); cursor: n-resize; }
.south { bottom: 0; left: 0; right: 0; height: var(--frame-edge); cursor: s-resize; }
.west  { top: 0; bottom: 0; left: 0; width: var(--frame-edge); cursor: w-resize; }
.east  { top: 0; bottom: 0; right: 0; width: var(--frame-edge); cursor: e-resize; }

/* 角落握把比邊寬一倍才好抓。上限＝不能碰到標題列的按鈕：最右邊的關閉鈕距邊框
   右緣 8px，所以 14px 還留餘裕。凸出圓角的部分由 overflow:hidden 裁掉。 */
.rh.nw, .rh.ne, .rh.sw, .rh.se { width: 14px; height: 14px; }
.nw { top: 0; left: 0; cursor: nw-resize; }
.ne { top: 0; right: 0; cursor: ne-resize; }
.sw { bottom: 0; left: 0; cursor: sw-resize; }
.se { bottom: 0; right: 0; cursor: se-resize; }
</style>

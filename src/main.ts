import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "./styles/main.css";

// WebView2 自帶的瀏覽器快捷鍵（F3／Ctrl+F 尋找、Ctrl+P 列印、Ctrl+J 下載、Alt+左右上下頁……）
// 在桌面程式裡全是誤觸來源，一律擋掉。
// 用白名單而不是列黑名單：WebView2 哪一版多加一顆，黑名單就漏一顆。
// 只 preventDefault、不 stopPropagation——程式自己的按鍵處理（Enter、Esc）照常收得到。
// ★F5／Ctrl+R 也不放行：重新整理會洗掉整份帳號清單。它們另外還有後端那一層
// （lib.rs 的 swallow_refresh_keys）——原生加速鍵不保證會先經過這裡。
// ★縮放（Ctrl+加減、Ctrl+滾輪）不必在這裡管：Tauri 2 的 zoomHotkeysEnabled 預設就是關的。
const EDIT_KEYS = new Set(["c", "v", "x", "a", "z", "y"]);
const CARET_KEYS = new Set([
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "Home",
  "End",
  "Backspace",
  "Delete",
  // Ctrl+Insert＝舊式的複製；Shift+Insert 沒帶 Ctrl，本來就不會進到這裡
  "Insert",
]);
document.addEventListener(
  "keydown",
  (e) => {
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    // AltGr 在 Windows 上會同時回報 Ctrl 與 Alt：那是在打字（歐洲鍵盤的 @、{ 都靠它），不是快捷鍵
    if (e.getModifierState("AltGraph")) return;
    const fkey = /^F\d{1,2}$/.test(e.key);
    // 沒帶 Ctrl／Alt 的一般按鍵是打字與程式自己的操作，不是瀏覽器快捷鍵
    if (!fkey && !e.ctrlKey && !e.altKey && !e.metaKey) return;
    // Shift+F10 是鍵盤版的右鍵
    if (e.key === "F10" && e.shiftKey && !e.ctrlKey && !e.altKey) return;
    // 開發時留著 DevTools（正式版本來就沒有 DevTools，這條不影響）
    if (import.meta.env.DEV && (e.key === "F12" || (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "i"))) return;
    if (e.altKey && !e.ctrlKey && !e.metaKey) {
      // 歸作業系統管的：Alt+F4 關視窗、Alt+空白鍵叫系統選單（這個視窗沒有標題列，鍵盤只剩這條路）、
      // Alt+數字鍵盤打特殊字元
      if (e.key === "F4" || e.code === "Space" || /^Numpad\d$/.test(e.code)) return;
    }
    if (e.ctrlKey && !e.altKey && !e.metaKey) {
      const k = e.key.toLowerCase();
      // Ctrl+方向鍵／Backspace 這類逐字移動與刪除，帶 Shift 是逐字選取
      if (CARET_KEYS.has(e.key)) return;
      // 複製貼上復原。帶 Shift 的只放行重做（Z）與貼成純文字（V）——
      // 其餘的 Ctrl+Shift 組合是瀏覽器功能的地盤（例如 Ctrl+Shift+C 是 DevTools 的選取元素）
      if (EDIT_KEYS.has(k) && (!e.shiftKey || k === "z" || k === "v")) return;
    }
    e.preventDefault();
  },
  true,
);

createApp(App).use(createPinia()).mount("#app");

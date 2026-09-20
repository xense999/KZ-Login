import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

// 主視窗的縮小鈕是收進通知列還是一般最小化。真正決定的是後端（圖示跟旗標都在它那邊），
// 這裡只負責記住選擇並告訴它；後端沒收到之前一律是一般最小化。
const KEY = "kusei:minimize_to_tray";

const current = ref(localStorage.getItem(KEY) === "1");

function push() {
  return invoke("set_minimize_to_tray", { on: current.value });
}

export function useMinimizeMode() {
  async function setMinimizeToTray(on: boolean) {
    const before = current.value;
    current.value = on;
    try {
      await push();
      localStorage.setItem(KEY, on ? "1" : "0");
    } catch (e) {
      current.value = before;
      throw e;
    }
  }
  // 啟動時把記住的選擇推給後端。推不成＝後端還是一般最小化，開關就得跟著顯示「關」，
  // 不然畫面說開著、行為卻是關的。localStorage 不動：下次啟動再試一次
  async function syncMinimizeMode() {
    try {
      await push();
    } catch (e) {
      current.value = false;
      throw e;
    }
  }
  return { minimizeToTray: current, setMinimizeToTray, syncMinimizeMode };
}

import { ref } from "vue";

// 主畫面右下角那顆按鈕要做什麼。設定頁的「偏好設定」切換，主畫面跟著換。
const KEY = "kusei:main_action";
export type MainAction = "proxy" | "game";

const current = ref<MainAction>(
  localStorage.getItem(KEY) === "game" ? "game" : "proxy"
);

export function useMainAction() {
  function setMainAction(a: MainAction) {
    current.value = a;
    localStorage.setItem(KEY, a);
  }
  return { mainAction: current, setMainAction };
}

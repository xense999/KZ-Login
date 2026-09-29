import { ref } from "vue";

// 快速登入照哪個解析度找遊戲登入框。null＝自動，照遊戲視窗大小算。
// 遊戲開了延伸介面後，選完角色視窗會變大、登入畫面卻還是原尺寸從左上角畫，
// 這時照視窗大小會點到空白處，要改用玩家在遊戲裡設的解析度。
const KEY = "kusei:game_resolution";

export const GAME_RESOLUTIONS: ReadonlyArray<readonly [number, number]> = [
  [1024, 768],
  [1280, 720],
  [1366, 768],
  [1920, 1080],
  [1920, 1200],
  [2560, 1440],
];

function load(): readonly [number, number] | null {
  const raw = localStorage.getItem(KEY);
  return GAME_RESOLUTIONS.find(([w, h]) => `${w}x${h}` === raw) ?? null;
}

const current = ref<readonly [number, number] | null>(load());

export function useGameResolution() {
  function setGameResolution(r: readonly [number, number] | null) {
    current.value = r;
    if (r) localStorage.setItem(KEY, `${r[0]}x${r[1]}`);
    else localStorage.removeItem(KEY);
  }
  return { gameResolution: current, setGameResolution };
}

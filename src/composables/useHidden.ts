import { ref } from "vue";

const KEY = "kusei:hidden_features";

// 功能 id → 顯示名稱。加隱藏功能時這裡與 Rust 端 hidden.rs 的對照表各補一列。
export const FEATURE_NAMES: Record<string, string> = {
  export: "批次匯出子帳號清單",
  export_otp: "批次匯出子帳號密碼",
};

function load(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((v) => typeof v === "string") : [];
  } catch {
    return [];
  }
}

const features = ref<string[]>(load());

export function useHidden() {
  function isUnlocked(id: string) {
    return features.value.includes(id);
  }
  // 同一組密鑰再輸入一次就關掉——不必為了「收起來」另外做一套 UI。
  function toggle(id: string): boolean {
    const on = !isUnlocked(id);
    features.value = on
      ? [...features.value, id]
      : features.value.filter((f) => f !== id);
    localStorage.setItem(KEY, JSON.stringify(features.value));
    return on;
  }
  return { features, isUnlocked, toggle };
}

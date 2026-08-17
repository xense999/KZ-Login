import { ref } from "vue";

// 全域浮動提示（單例）。任何地方 import { toast } 呼叫即顯示，逾時自動消失；
// 連續呼叫會換文字並重置倒數。顯示端＝App.vue 掛載的 ToastPop.vue（唯一）。
export type ToastKind = "info" | "error";

const message = ref("");
const kind = ref<ToastKind>("info");
const visible = ref(false);
let timer: ReturnType<typeof setTimeout> | undefined;

export function toast(msg: string, opts?: { kind?: ToastKind; ms?: number }) {
  message.value = msg;
  kind.value = opts?.kind ?? "info";
  visible.value = true;
  clearTimeout(timer);
  timer = setTimeout(() => (visible.value = false), opts?.ms ?? 3000);
}

// 供 ToastPop.vue 讀取顯示狀態；一般頁面只需要 toast()。
export function useToast() {
  return { message, kind, visible };
}

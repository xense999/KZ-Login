import { ref } from "vue";

const THEME_KEY = "kusei:theme";
export type Theme = "neutral" | "dark";

const current = ref<Theme>(
  (localStorage.getItem(THEME_KEY) as Theme) || "neutral"
);

function apply(t: Theme) {
  if (t === "dark") {
    document.documentElement.setAttribute("data-theme", "dark");
  } else {
    document.documentElement.removeAttribute("data-theme");
  }
}

apply(current.value);

export function useTheme() {
  function setTheme(t: Theme) {
    current.value = t;
    localStorage.setItem(THEME_KEY, t);
    apply(t);
  }
  return { theme: current, setTheme };
}

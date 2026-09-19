import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { resolve } from "node:path";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    // 帳號瀏覽器的殼層是自己的 webview，所以自己一頁：不跟主視窗共用進入點，
    // 免得殼層一載入就跑主 app 的啟動流程（session 檢查、GGM 更新）。
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        browser: resolve(__dirname, "browser.html"),
      },
    },
  },
});

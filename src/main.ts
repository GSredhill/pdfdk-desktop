import { createApp } from "vue";
import "./styles/tokens.css";

async function boot() {
  // `vite dev` in a plain browser: no Tauri bridge → preview with a mock
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    await import("./dev/mock");
  }
  const { default: App } = await import("./App.vue");
  createApp(App).mount("#app");
}

boot();

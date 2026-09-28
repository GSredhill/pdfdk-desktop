import { createApp } from "vue";
import "./styles/tokens.css";

async function boot() {
  // `vite dev` in a plain browser: no Tauri bridge → preview with a mock
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    await import("./dev/mock");
  }
  // a desktop widget window loads the same bundle with ?widget=<id>
  if (new URLSearchParams(location.search).get("widget")) {
    const { default: WidgetApp } = await import("./WidgetApp.vue");
    createApp(WidgetApp).mount("#app");
    return;
  }
  const { default: App } = await import("./App.vue");
  createApp(App).mount("#app");
}

boot();

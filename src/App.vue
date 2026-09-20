<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { t, lang } from "./i18n";
import { emptyAuth, type AppConfig, type AuthState, type ToolDefinition } from "./types";
import LoginView from "./components/LoginView.vue";
import ToolsView from "./components/ToolsView.vue";
import ActivityView from "./components/ActivityView.vue";
import SettingsView from "./components/SettingsView.vue";
import QuickAction from "./components/QuickAction.vue";

type View = "tools" | "activity" | "settings";

const loading = ref(true);
const auth = ref<AuthState>(emptyAuth());
const config = ref<AppConfig | null>(null);
const tools = ref<ToolDefinition[]>([]);
const toolsLoading = ref(false);
const view = ref<View>("tools");
const siteBase = ref("https://pdf.dk");
const version = ref("");
const update = ref<Update | null>(null);
const updating = ref(false);
const droppedFiles = ref<string[]>([]);
const dragOver = ref(false);

const signedIn = computed(() => auth.value.isAuthenticated);
const planLabel = computed(() => {
  const p = (auth.value.plan || "free").toLowerCase();
  if (p === "team" || p === "superadmin" || p === "enterprise") return "PRO";
  return p.toUpperCase();
});
const usageText = computed(() => {
  if (auth.value.isUnlimited) return t("unlimited");
  if (auth.value.jobsLimit) return `${auth.value.jobsUsed ?? 0} / ${auth.value.jobsLimit} ${t("runsThisMonth")}`;
  return "";
});

// ---- theme + language follow the config
function applyTheme(theme: string) {
  const dark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}
const mq = window.matchMedia("(prefers-color-scheme: dark)");
const onScheme = () => applyTheme(config.value?.general.theme || "system");
watch(() => config.value?.general.theme, (th) => applyTheme(th || "system"));
watch(() => config.value?.general.language, (l) => { lang.value = l === "en" ? "en" : "da"; document.documentElement.lang = lang.value; });

// ---- data
async function loadConfig() {
  config.value = await invoke<AppConfig>("get_config");
}
async function loadTools(refresh = false) {
  toolsLoading.value = true;
  try {
    tools.value = await invoke<ToolDefinition[]>("get_available_tools", { refresh });
  } finally {
    toolsLoading.value = false;
  }
}
async function afterSignIn() {
  await loadConfig();
  // always refetch the catalogue on start; the Rust side falls back to the cached copy when offline
  await loadTools(true);
  await invoke("start_watchers").catch((e) => console.error(e));
  const pending = await invoke<string[]>("take_pending_files");
  if (pending.length) droppedFiles.value = pending;
}
async function refreshAuth() {
  auth.value = await invoke<AuthState>("check_auth");
}
async function signOut() {
  await invoke("logout");
  auth.value = emptyAuth();
  view.value = "tools";
}
async function saveConfig(next: AppConfig) {
  config.value = next;
  await invoke("save_config", { newConfig: next });
}

// ---- updates
async function checkForUpdates(): Promise<boolean> {
  try {
    const u = await check();
    if (u) { update.value = u; return true; }
  } catch (e) {
    console.log("updater:", e);
  }
  return false;
}
async function installUpdate() {
  if (!update.value) return;
  updating.value = true;
  try {
    await update.value.downloadAndInstall();
    await relaunch();
  } catch (e) {
    console.error("update failed", e);
    updating.value = false;
    await openUrl(`${siteBase.value}/desktop`);
  }
}

// ---- files from the OS: drag-drop on the window, dock drop, open-with
let unlistenOpened: UnlistenFn | null = null;
let unlistenDrag: UnlistenFn | null = null;

onMounted(async () => {
  document.documentElement.lang = "da";
  mq.addEventListener("change", onScheme);
  version.value = await getVersion();
  siteBase.value = await invoke<string>("get_site_base");
  await loadConfig();
  applyTheme(config.value?.general.theme || "system");
  lang.value = config.value?.general.language === "en" ? "en" : "da";

  await refreshAuth();
  if (signedIn.value) await afterSignIn();
  loading.value = false;
  checkForUpdates();

  unlistenOpened = await listen<string[]>("files-opened", (e) => {
    if (signedIn.value) droppedFiles.value = [...droppedFiles.value, ...e.payload];
  });
  unlistenDrag = await getCurrentWebview().onDragDropEvent((e) => {
    if (e.payload.type === "enter" || e.payload.type === "over") dragOver.value = true;
    else if (e.payload.type === "leave") dragOver.value = false;
    else if (e.payload.type === "drop") {
      dragOver.value = false;
      if (signedIn.value && e.payload.paths.length) droppedFiles.value = e.payload.paths;
    }
  });
});
onBeforeUnmount(() => {
  mq.removeEventListener("change", onScheme);
  unlistenOpened?.();
  unlistenDrag?.();
});
</script>

<template>
  <div class="app" :class="{ 'drag-over': dragOver }">
    <div v-if="loading" class="boot">
      <div class="spin"></div>
    </div>

    <LoginView
      v-else-if="!signedIn"
      :site-base="siteBase"
      @signed-in="(a) => { auth = a; afterSignIn(); }"
    />

    <template v-else>
      <header class="hdr">
        <div class="hdr-l">
          <span class="pdk-brand hdr-brand">pdf<i class="pdk-dot"></i>dk</span>
          <span class="hdr-sub">Desktop</span>
        </div>
        <nav class="hdr-nav">
          <button class="nav-btn" :class="{ on: view === 'tools' }" @click="view = 'tools'">{{ t("tools") }}</button>
          <button class="nav-btn" :class="{ on: view === 'activity' }" @click="view = 'activity'">{{ t("activity") }}</button>
          <button class="nav-btn" :class="{ on: view === 'settings' }" @click="view = 'settings'">{{ t("settings") }}</button>
        </nav>
        <div class="hdr-r">
          <button v-if="update" class="btn btn-sm btn-primary" :disabled="updating" @click="installUpdate">
            {{ updating ? t("updating") : t("updateTo", { v: update.version }) }}
          </button>
          <span v-if="usageText" class="hdr-usage mono">{{ usageText }}</span>
          <span class="pill" :class="auth.isPro ? 'pill-brand' : 'pill-muted'">{{ planLabel }}</span>
          <button class="btn btn-ghost btn-sm" :title="auth.user?.email" @click="openUrl(siteBase)">{{ t("openWebsite") }}</button>
        </div>
      </header>

      <main class="main">
        <ToolsView
          v-if="view === 'tools'"
          :tools="tools"
          :config="config"
          :loading="toolsLoading"
          :site-base="siteBase"
          @reload="loadTools(true)"
          @config-changed="loadConfig"
        />
        <ActivityView v-else-if="view === 'activity'" :active="view === 'activity'" />
        <SettingsView
          v-else
          :config="config"
          :auth="auth"
          :version="version"
          :site-base="siteBase"
          @save="saveConfig"
          @sign-out="signOut"
          @check-updates="checkForUpdates"
        />
      </main>

      <div v-if="dragOver" class="drop-veil"><div class="drop-msg">{{ t("dropHint") }}</div></div>

      <QuickAction
        v-if="droppedFiles.length"
        :files="droppedFiles"
        :tools="tools"
        :config="config"
        @close="droppedFiles = []"
        @config-changed="loadConfig"
      />
    </template>
  </div>
</template>

<style>
.app { height: 100vh; display: flex; flex-direction: column; position: relative; }
.boot { flex: 1; display: grid; place-items: center; }

.hdr {
  display: flex; align-items: center; gap: 18px; height: 56px; padding: 0 18px; flex: none;
  background: var(--card); border-bottom: 1px solid var(--cardline);
}
.hdr-l { display: flex; align-items: baseline; gap: 8px; min-width: 150px; }
.hdr-brand { font-size: 21px; }
.hdr-sub { font-size: 12px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); }
.hdr-nav { display: flex; gap: 2px; background: var(--inset); border: 1px solid var(--cardline); border-radius: 11px; padding: 3px; }
.nav-btn { height: 30px; padding: 0 14px; border: 0; border-radius: 8px; background: transparent; color: var(--muted); font-weight: 700; font-size: 12.5px; cursor: pointer; }
.nav-btn:hover { color: var(--text); }
.nav-btn.on { background: var(--card); color: var(--text); box-shadow: var(--shadow); }
.hdr-r { margin-left: auto; display: flex; align-items: center; gap: 10px; }
.hdr-usage { font-size: 12px; color: var(--muted); }

.main { flex: 1; min-height: 0; overflow: auto; }

.drop-veil { position: absolute; inset: 0; background: var(--brandSoft); border: 3px dashed var(--brand); display: grid; place-items: center; z-index: 40; pointer-events: none; }
.drop-msg { background: var(--card); border: 1px solid var(--brandLine); border-radius: 14px; padding: 14px 20px; font-weight: 800; box-shadow: var(--shadow); }
</style>

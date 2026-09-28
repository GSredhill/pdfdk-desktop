<script setup lang="ts">
import { ref, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { t, bi } from "../i18n";
import { CATEGORY_ORDER, CATEGORY_VAR, type AppConfig, type ToolConfig, type ToolDefinition, type WidgetConfig } from "../types";
import OptionsModal from "./OptionsModal.vue";

const props = defineProps<{ tools: ToolDefinition[]; config: AppConfig | null; loading: boolean; siteBase: string }>();
const emit = defineEmits<{ (e: "reload"): void; (e: "config-changed"): void }>();

const query = ref("");
const onlyEnabled = ref(false);
const optionsFor = ref<{ tool: ToolDefinition; entry: ToolConfig | null; widget?: WidgetConfig } | null>(null);
const widgetNotice = ref("");

// ---- desktop widgets (0.4.0)
const widgets = computed(() => props.config?.widgets ?? []);
function widgetTool(w: WidgetConfig): ToolDefinition | undefined {
  return props.tools.find((x) => x.id === w.toolId);
}
async function createWidget(tool: ToolDefinition) {
  error.value = "";
  try {
    await invoke("widget_create", { toolId: tool.id });
    emit("config-changed");
    widgetNotice.value = t("widgetCreated");
    setTimeout(() => (widgetNotice.value = ""), 2500);
  } catch (e) {
    error.value = String(e);
  }
}
async function removeWidget(w: WidgetConfig) {
  await invoke("widget_remove", { id: w.id }).catch((e) => (error.value = String(e)));
  emit("config-changed");
}
async function showWidget(w: WidgetConfig) {
  await invoke("widget_show", { id: w.id }).catch((e) => (error.value = String(e)));
}
async function setWidgetCombine(w: WidgetConfig, combine: boolean) {
  await invoke("widget_update", { id: w.id, options: w.options, combine }).catch((e) => (error.value = String(e)));
  emit("config-changed");
}
const busyTool = ref("");
const error = ref("");

const enabledCount = computed(() => props.config?.tools.filter((x) => x.enabled).length ?? 0);

const groups = computed(() => {
  const q = query.value.trim().toLowerCase();
  const list = props.tools.filter((tool) => {
    if (onlyEnabled.value && !isEnabled(tool.id)) return false;
    if (!q) return true;
    return (bi(tool.name) + " " + bi(tool.description) + " " + tool.id + " " + tool.accepts.join(" ")).toLowerCase().includes(q);
  });
  const known = CATEGORY_ORDER.filter((c) => list.some((x) => x.category === c));
  const other = [...new Set(list.map((x) => x.category))].filter((c) => !CATEGORY_ORDER.includes(c));
  return [...known, ...other].map((cat) => ({
    cat,
    label: bi(list.find((x) => x.category === cat)?.category_label) || cat,
    accent: `var(${CATEGORY_VAR[cat] || "--brand"})`,
    tools: list.filter((x) => x.category === cat),
  }));
});

/** every watched folder of a tool (a tool can have several, each with its own options) */
function entries(id: string): ToolConfig[] {
  return props.config?.tools.filter((x) => x.id === id && x.enabled) ?? [];
}
function isEnabled(id: string): boolean {
  return entries(id).length > 0;
}
/** the options drag-and-drop / Open with use: the tool's first entry */
function defaultOptions(id: string): Record<string, unknown> {
  return props.config?.tools.find((x) => x.id === id)?.options || {};
}
function shortFolder(p: string): string {
  const home = /^\/Users\/[^/]+|^C:\\Users\\[^\\]+/.exec(p)?.[0];
  return home ? p.replace(home, "~") : p;
}
function visibleOptions(tool: ToolDefinition) {
  return tool.options.filter((o) => o.type !== "hidden");
}
function optionSummary(tool: ToolDefinition, values: Record<string, unknown>): string {
  return visibleOptions(tool)
    .filter((o) => o.type !== "text" || !o.secret)
    .map((o) => {
      const v = values[o.name] ?? o.default;
      if (o.type === "boolean") return v ? bi(o.label) : "";
      if (o.type === "select") {
        const c = o.choices.find((ch) => String(ch.value) === String(v));
        return c ? bi(c.label) : String(v ?? "");
      }
      return v == null || v === "" ? "" : `${bi(o.label)}: ${v}`;
    })
    .filter(Boolean)
    .join(" · ");
}

/** pick a folder: for an existing entry (change) or a new one (enable / add another) */
async function chooseFolder(tool: ToolDefinition, entry: ToolConfig | null = null) {
  const selected = await open({ directory: true, multiple: false, title: bi(tool.name) });
  if (!selected || typeof selected !== "string") return;
  busyTool.value = tool.id;
  error.value = "";
  try {
    await invoke("enable_tool", { toolId: tool.id, folderPath: selected, key: entry?.key ?? null });
    emit("config-changed");
  } catch (e) {
    error.value = String(e);
  } finally {
    busyTool.value = "";
  }
}
function outMode(entry: ToolConfig): "subfolder" | "same" | "custom" {
  if (typeof entry.outputMode === "object") return "custom";
  return entry.outputMode === "same-folder" ? "same" : "subfolder";
}
function outTitle(entry: ToolConfig): string {
  return typeof entry.outputMode === "object" ? entry.outputMode.custom : "";
}
async function setOutput(tool: ToolDefinition, entry: ToolConfig, mode: string) {
  let path: string | null = null;
  if (mode === "custom") {
    const selected = await open({ directory: true, multiple: false, title: bi(tool.name) });
    if (!selected || typeof selected !== "string") { emit("config-changed"); return; }   // re-render the old value
    path = selected;
  }
  busyTool.value = tool.id;
  error.value = "";
  try {
    await invoke("update_tool_output", { key: entry.key, mode, path });
    emit("config-changed");
  } catch (e) {
    error.value = String(e);
  } finally {
    busyTool.value = "";
  }
}
async function disable(tool: ToolDefinition, entry: ToolConfig) {
  busyTool.value = tool.id;
  try {
    await invoke("disable_tool", { key: entry.key });
    emit("config-changed");
  } finally {
    busyTool.value = "";
  }
}
</script>

<template>
  <div class="tv">
    <div class="tv-top">
      <div>
        <h2 class="tv-h">{{ t("watchedFolders") }}</h2>
        <p class="tv-p">{{ t("watchedFoldersDesc") }} <span class="tv-hint">{{ t("dropHint") }}</span></p>
      </div>
      <div class="tv-tools">
        <input v-model="query" class="input tv-search" type="search" :placeholder="t('filterTools')" />
        <div class="seg">
          <button class="seg-btn" :class="{ on: !onlyEnabled }" @click="onlyEnabled = false">{{ t("showAll") }}</button>
          <button class="seg-btn" :class="{ on: onlyEnabled }" @click="onlyEnabled = true">{{ t("onlyEnabled") }} <span class="mono">{{ enabledCount }}</span></button>
        </div>
      </div>
    </div>

    <div v-if="error" class="tv-err">{{ error }}</div>

    <div v-if="!tools.length && !loading" class="tv-empty card">
      <p>{{ t("noTools") }}</p>
      <button class="btn" @click="emit('reload')">{{ t("refreshTools") }}</button>
    </div>

    <section v-if="widgets.length" class="tv-group tv-widgets">
      <h3 class="tv-cat"><span class="tv-cat-dot" style="background: var(--brand);"></span>{{ t("widgets") }}</h3>
      <p class="tv-p tv-widgets-p">{{ t("widgetsDesc") }}</p>
      <div class="tv-wgrid">
        <div v-for="w in widgets" :key="w.id" class="tw card" :style="{ '--acc': `var(${CATEGORY_VAR[widgetTool(w)?.category || ''] || '--brand'})` }">
          <div class="tw-head">
            <span class="tw-dot"></span>
            <span class="tw-name">{{ widgetTool(w) ? bi(widgetTool(w)!.name) : w.toolId }}</span>
          </div>
          <div v-if="widgetTool(w) && optionSummary(widgetTool(w)!, w.options)" class="tc-folder-opts">{{ optionSummary(widgetTool(w)!, w.options) }}</div>
          <label v-if="widgetTool(w)?.file_field.endsWith('[]')" class="tw-combine">
            <input type="checkbox" :checked="w.combine" @change="setWidgetCombine(w, ($event.target as HTMLInputElement).checked)" />
            <span>{{ t("widgetCombine") }}</span>
          </label>
          <div class="tw-actions">
            <button class="btn btn-sm btn-ghost" @click="showWidget(w)">{{ t("widgetShow") }}</button>
            <button v-if="widgetTool(w) && visibleOptions(widgetTool(w)!).length" class="btn btn-sm btn-ghost" @click="optionsFor = { tool: widgetTool(w)!, entry: null, widget: w }">{{ t("options") }}</button>
            <button class="btn btn-sm btn-ghost tw-remove" @click="removeWidget(w)">{{ t("removeFolder") }}</button>
          </div>
        </div>
      </div>
    </section>

    <section v-for="g in groups" :key="g.cat" class="tv-group" :style="{ '--acc': g.accent }">
      <h3 class="tv-cat"><span class="tv-cat-dot"></span>{{ g.label }}</h3>
      <div class="tv-grid">
        <article v-for="tool in g.tools" :key="tool.id" class="tc card" :class="{ on: isEnabled(tool.id), busy: busyTool === tool.id }">
          <div class="tc-head">
            <div class="tc-ic"><span></span></div>
            <div class="tc-txt">
              <h4 class="tc-name">{{ bi(tool.name) }}</h4>
              <p class="tc-desc">{{ bi(tool.description) }}</p>
            </div>
            <button v-if="visibleOptions(tool).length && !isEnabled(tool.id)" class="tc-gear" :title="t('defaultOptions')" @click="optionsFor = { tool, entry: null }">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></svg>
            </button>
          </div>

          <div class="tc-meta">
            <span class="pill pill-muted">{{ t("accepts") }} {{ tool.accepts.map((e) => "." + e).join(" ") }}</span>
            <span v-if="!isEnabled(tool.id) && optionSummary(tool, defaultOptions(tool.id))" class="tc-opts">{{ optionSummary(tool, defaultOptions(tool.id)) }}</span>
          </div>

          <div v-for="entry in entries(tool.id)" :key="entry.key" class="tc-folder">
            <div class="tc-folder-row">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
              <span class="mono tc-path" :title="entry.folderPath || ''">{{ shortFolder(entry.folderPath || "") }}</span>
              <button v-if="visibleOptions(tool).length" class="tc-ib" :title="t('folderOptions')" @click="optionsFor = { tool, entry }">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></svg>
              </button>
              <button class="tc-ib" :title="t('changeFolder')" @click="chooseFolder(tool, entry)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M17 1l4 4-4 4"/><path d="M3 11V9a4 4 0 0 1 4-4h14"/><path d="M7 23l-4-4 4-4"/><path d="M21 13v2a4 4 0 0 1-4 4H3"/></svg>
              </button>
              <button class="tc-ib tc-ib-danger" :title="t('removeFolder')" @click="disable(tool, entry)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 6L6 18M6 6l12 12"/></svg>
              </button>
            </div>
            <div v-if="optionSummary(tool, entry.options)" class="tc-folder-opts">{{ optionSummary(tool, entry.options) }}</div>
            <div class="tc-folder-out">
              <span>{{ t("outputWhere") }}</span>
              <select class="tc-out" :value="outMode(entry)" :title="outTitle(entry)" @change="setOutput(tool, entry, ($event.target as HTMLSelectElement).value)">
                <option value="subfolder">{{ t("outputSubfolder") }}</option>
                <option value="same">{{ t("outputSame") }}</option>
                <option value="custom">{{ typeof entry.outputMode === "object" ? shortFolder(entry.outputMode.custom) : t("outputCustom") }}</option>
              </select>
            </div>
          </div>

          <div class="tc-actions">
            <button v-if="isEnabled(tool.id)" class="btn btn-sm btn-ghost" @click="chooseFolder(tool)">+ {{ t("addFolder") }}</button>
            <button v-else class="btn btn-sm btn-primary" @click="chooseFolder(tool)">{{ t("enable") }}</button>
            <button class="btn btn-sm btn-ghost tc-widget" :title="t('widgetCreate')" @click="createWidget(tool)">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="8" height="8" rx="2"/><rect x="13" y="3" width="8" height="8" rx="2"/><rect x="3" y="13" width="8" height="8" rx="2"/><path d="M17 14v6M14 17h6"/></svg>
              {{ t("widgetCreate") }}
            </button>
            <a class="tc-web" href="#" :title="siteBase + bi(tool.web_path)" @click.prevent="openUrl(siteBase + bi(tool.web_path))">pdf.dk ↗</a>
          </div>
        </article>
      </div>
    </section>

    <transition name="tv-toast"><div v-if="widgetNotice" class="tv-toast">{{ widgetNotice }}</div></transition>

    <OptionsModal
      v-if="optionsFor"
      :tool="optionsFor.tool"
      :values="optionsFor.widget?.options || optionsFor.entry?.options || defaultOptions(optionsFor.tool.id)"
      :entry-key="optionsFor.widget ? 'widget:' + optionsFor.widget.id : (optionsFor.entry?.key ?? null)"
      @close="optionsFor = null"
      @saved="optionsFor = null; emit('config-changed')"
    />
  </div>
</template>

<style scoped>
.tv { padding: 22px 24px 40px; max-width: 1180px; margin: 0 auto; }
.tv-widgets-p { margin-bottom: 10px; }
.tv-wgrid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; }
.tw { padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; border-color: color-mix(in srgb, var(--acc) 40%, var(--cardline)); }
.tw-head { display: flex; align-items: center; gap: 8px; }
.tw-dot { width: 9px; height: 9px; border-radius: 50%; background: var(--acc); }
.tw-name { font-weight: 800; font-size: 13.5px; }
.tw-combine { display: flex; align-items: center; gap: 7px; font-size: 12px; color: var(--muted); cursor: pointer; }
.tw-actions { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 2px; }
.tw-remove { color: var(--bad); }
.tc-widget { display: inline-flex; align-items: center; gap: 5px; }
.tc-widget svg { width: 14px; height: 14px; }
.tv-toast { position: fixed; left: 50%; bottom: 26px; transform: translateX(-50%); padding: 9px 14px; border-radius: 10px; background: var(--text); color: var(--bg); font-size: 12.5px; font-weight: 700; box-shadow: var(--shadow); z-index: 40; }
.tv-toast-enter-active, .tv-toast-leave-active { transition: opacity .18s, transform .18s; }
.tv-toast-enter-from, .tv-toast-leave-to { opacity: 0; transform: translate(-50%, 6px); }
.tv-top { display: flex; align-items: flex-start; justify-content: space-between; gap: 20px; margin-bottom: 8px; }
.tv-h { font-size: 20px; }
.tv-p { margin: 4px 0 0; color: var(--muted); max-width: 620px; }
.tv-hint { color: var(--faint); }
.tv-tools { display: flex; align-items: center; gap: 10px; flex: none; }
.tv-search { width: 200px; height: 34px; }
.seg { display: flex; gap: 2px; background: var(--inset); border: 1px solid var(--cardline); border-radius: 10px; padding: 3px; }
.seg-btn { height: 26px; padding: 0 10px; border: 0; border-radius: 7px; background: transparent; color: var(--muted); font-weight: 700; font-size: 12px; cursor: pointer; display: inline-flex; gap: 6px; align-items: center; }
.seg-btn.on { background: var(--card); color: var(--text); box-shadow: var(--shadow); }
.tv-empty { margin-top: 20px; padding: 24px; display: flex; flex-direction: column; align-items: flex-start; gap: 12px; color: var(--muted); }

.tv-group { margin-top: 22px; }
.tv-cat { display: flex; align-items: center; gap: 8px; font-size: 11.5px; letter-spacing: .09em; text-transform: uppercase; color: var(--muted); margin-bottom: 10px; }
.tv-cat-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--acc); }
.tv-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 12px; }

.tc { padding: 14px 14px 12px; display: flex; flex-direction: column; gap: 10px; transition: border-color .15s, box-shadow .15s; }
.tc:hover { border-color: var(--acc); }
.tc.on { border-color: var(--acc); box-shadow: 0 0 0 3px color-mix(in srgb, var(--acc) 12%, transparent), var(--shadow); }
.tc.busy { opacity: .6; pointer-events: none; }
.tc-head { display: flex; gap: 12px; align-items: flex-start; }
.tc-ic { width: 40px; height: 40px; flex: none; border-radius: 12px; display: grid; place-items: center; background: color-mix(in srgb, var(--acc) 12%, transparent); }
.tc-ic span { width: 11px; height: 11px; border-radius: 50%; background: var(--acc); }
.tc-txt { min-width: 0; flex: 1; }
.tc-name { font-size: 14px; }
.tc-desc { margin: 2px 0 0; font-size: 12.5px; color: var(--muted); display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.tc-gear { width: 30px; height: 30px; flex: none; border: 0; border-radius: 8px; background: transparent; color: var(--muted); cursor: pointer; display: grid; place-items: center; }
.tc-gear svg { width: 17px; height: 17px; }
.tc-gear:hover { background: var(--inset); color: var(--text); }
.tc-meta { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; font-size: 12px; color: var(--muted); }
.tc-opts { color: var(--text2); }
.tc-folder { padding: 7px 8px 7px 10px; border-radius: 9px; background: var(--inset); color: var(--muted); font-size: 12px; }
.tc-folder-row { display: flex; align-items: center; gap: 6px; }
.tc-folder-row > svg { width: 15px; height: 15px; flex: none; color: var(--acc); margin-right: 2px; }
.tc-path { color: var(--text2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; min-width: 0; }
.tc-folder-opts { margin: 4px 0 0 23px; color: var(--muted); font-size: 11.5px; }
.tc-folder-out { margin: 4px 0 0 23px; display: flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--muted); }
.tc-out { height: 22px; font-size: 11.5px; border-radius: 6px; border: 1px solid var(--cardline); background: var(--card); color: var(--text2); padding: 0 4px; max-width: 200px; }
.tc-ib { width: 24px; height: 24px; flex: none; border: 0; border-radius: 6px; background: transparent; color: var(--faint); cursor: pointer; display: grid; place-items: center; }
.tc-ib svg { width: 14px; height: 14px; }
.tc-ib:hover { background: var(--card); color: var(--text); }
.tc-ib-danger:hover { color: var(--bad); }
.tv-err { margin-top: 10px; background: var(--badSoft); color: var(--bad); border-radius: 9px; padding: 9px 11px; font-size: 12.5px; }
.tc-actions { display: flex; align-items: center; gap: 8px; margin-top: auto; }
.tc-web { margin-left: auto; font-size: 11.5px; color: var(--faint); }
.tc-web:hover { color: var(--brand); }
</style>

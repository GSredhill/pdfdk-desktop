<script setup lang="ts">
import { ref, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { t, bi } from "../i18n";
import { CATEGORY_ORDER, CATEGORY_VAR, type AppConfig, type ToolConfig, type ToolDefinition } from "../types";
import OptionsModal from "./OptionsModal.vue";

const props = defineProps<{ tools: ToolDefinition[]; config: AppConfig | null; loading: boolean; siteBase: string }>();
const emit = defineEmits<{ (e: "reload"): void; (e: "config-changed"): void }>();

const query = ref("");
const onlyEnabled = ref(false);
const optionsFor = ref<ToolDefinition | null>(null);
const busyTool = ref("");

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

function cfg(id: string): ToolConfig | undefined {
  return props.config?.tools.find((x) => x.id === id);
}
function isEnabled(id: string): boolean {
  return cfg(id)?.enabled ?? false;
}
function folder(id: string): string {
  return cfg(id)?.folderPath ?? "";
}
function shortFolder(p: string): string {
  const home = /^\/Users\/[^/]+|^C:\\Users\\[^\\]+/.exec(p)?.[0];
  return home ? p.replace(home, "~") : p;
}
function visibleOptions(tool: ToolDefinition) {
  return tool.options.filter((o) => o.type !== "hidden");
}
function optionSummary(tool: ToolDefinition): string {
  const values = cfg(tool.id)?.options || {};
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

async function chooseFolder(tool: ToolDefinition) {
  const selected = await open({ directory: true, multiple: false, title: bi(tool.name) });
  if (!selected || typeof selected !== "string") return;
  busyTool.value = tool.id;
  try {
    await invoke("enable_tool", { toolId: tool.id, folderPath: selected });
    emit("config-changed");
  } finally {
    busyTool.value = "";
  }
}
async function disable(tool: ToolDefinition) {
  busyTool.value = tool.id;
  try {
    await invoke("disable_tool", { toolId: tool.id });
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

    <div v-if="!tools.length && !loading" class="tv-empty card">
      <p>{{ t("noTools") }}</p>
      <button class="btn" @click="emit('reload')">{{ t("refreshTools") }}</button>
    </div>

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
            <button v-if="visibleOptions(tool).length" class="tc-gear" :title="t('options')" @click="optionsFor = tool">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></svg>
            </button>
          </div>

          <div class="tc-meta">
            <span class="pill pill-muted">{{ t("accepts") }} {{ tool.accepts.map((e) => "." + e).join(" ") }}</span>
            <span v-if="isEnabled(tool.id) && optionSummary(tool)" class="tc-opts">{{ optionSummary(tool) }}</span>
          </div>

          <div v-if="isEnabled(tool.id)" class="tc-folder">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
            <span class="mono tc-path" :title="folder(tool.id)">{{ shortFolder(folder(tool.id)) }}</span>
          </div>

          <div class="tc-actions">
            <template v-if="isEnabled(tool.id)">
              <button class="btn btn-sm" @click="chooseFolder(tool)">{{ t("changeFolder") }}</button>
              <button class="btn btn-sm btn-ghost btn-danger" @click="disable(tool)">{{ t("disable") }}</button>
            </template>
            <button v-else class="btn btn-sm btn-primary" @click="chooseFolder(tool)">{{ t("enable") }}</button>
            <a class="tc-web" href="#" :title="siteBase + bi(tool.web_path)" @click.prevent="openUrl(siteBase + bi(tool.web_path))">pdf.dk ↗</a>
          </div>
        </article>
      </div>
    </section>

    <OptionsModal
      v-if="optionsFor"
      :tool="optionsFor"
      :values="cfg(optionsFor.id)?.options || {}"
      @close="optionsFor = null"
      @saved="optionsFor = null; emit('config-changed')"
    />
  </div>
</template>

<style scoped>
.tv { padding: 22px 24px 40px; max-width: 1180px; margin: 0 auto; }
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
.tc-folder { display: flex; align-items: center; gap: 8px; padding: 8px 10px; border-radius: 9px; background: var(--inset); color: var(--muted); font-size: 12px; }
.tc-folder svg { width: 15px; height: 15px; flex: none; color: var(--acc); }
.tc-path { color: var(--text2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.tc-actions { display: flex; align-items: center; gap: 8px; margin-top: auto; }
.tc-web { margin-left: auto; font-size: 11.5px; color: var(--faint); }
.tc-web:hover { color: var(--brand); }
</style>

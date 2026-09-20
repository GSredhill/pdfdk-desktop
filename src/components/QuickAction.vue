<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { t, bi } from "../i18n";
import { CATEGORY_VAR, baseName, fileExt, type AppConfig, type FileResult, type ToolDefinition } from "../types";
import OptionsModal from "./OptionsModal.vue";

const props = defineProps<{ files: string[]; tools: ToolDefinition[]; config: AppConfig | null }>();
const emit = defineEmits<{ (e: "close"): void; (e: "config-changed"): void }>();

const running = ref(false);
const chosen = ref<ToolDefinition | null>(null);
const optionsFor = ref<ToolDefinition | null>(null);
const results = ref<FileResult[] | null>(null);

const exts = computed(() => [...new Set(props.files.map(fileExt))]);
const candidates = computed(() =>
  props.tools.filter((tool) => exts.value.every((e) => tool.accepts.map((a) => a.toLowerCase()).includes(e))),
);
function accent(tool: ToolDefinition) {
  return `var(${CATEGORY_VAR[tool.category] || "--brand"})`;
}
function savedOptions(tool: ToolDefinition) {
  return props.config?.tools.find((x) => x.id === tool.id)?.options || {};
}

async function run(tool: ToolDefinition) {
  chosen.value = tool;
  running.value = true;
  try {
    results.value = await invoke<FileResult[]>("process_files", { toolId: tool.id, paths: props.files });
  } catch (e) {
    results.value = props.files.map((f) => ({ input: f, output: null, error: String(e) }));
  } finally {
    running.value = false;
  }
}
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && !running.value && !optionsFor.value) emit("close");
}
onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div class="ov" @click.self="!running && emit('close')">
    <div class="qa card" role="dialog" aria-modal="true">
      <div class="qa-h">
        <div>
          <h3>{{ results ? bi(chosen?.name) : t("quickTitle") }}</h3>
          <p class="qa-sub">{{ t("quickSub", { n: files.length }) }}</p>
        </div>
        <button class="qa-x" :aria-label="t('close')" :disabled="running" @click="emit('close')">×</button>
      </div>

      <div class="qa-files">
        <span v-for="f in files.slice(0, 6)" :key="f" class="pill pill-muted mono" :title="f">{{ baseName(f) }}</span>
        <span v-if="files.length > 6" class="pill pill-muted">+{{ files.length - 6 }}</span>
      </div>

      <div v-if="!results && !running" class="qa-list">
        <p v-if="!candidates.length" class="qa-none">{{ t("noToolForFile") }}</p>
        <button v-for="tool in candidates" :key="tool.id" class="qa-item" :style="{ '--acc': accent(tool) }" @click="run(tool)">
          <span class="qa-dot"></span>
          <span class="qa-txt">
            <span class="qa-name">{{ bi(tool.name) }}</span>
            <span class="qa-desc">{{ bi(tool.description) }}</span>
          </span>
          <span
            v-if="tool.options.some((o) => o.type !== 'hidden')"
            class="qa-gear"
            role="button"
            :title="t('options')"
            @click.stop="optionsFor = tool"
          >⚙</span>
        </button>
      </div>

      <div v-else-if="running" class="qa-run">
        <div class="spin"></div>
        <span>{{ t("running") }}</span>
      </div>

      <div v-else class="qa-results">
        <div v-for="r in results" :key="r.input" class="qa-res">
          <span class="pill" :class="r.output ? 'pill-ok' : 'pill-bad'">{{ r.output ? t("done") : t("failed") }}</span>
          <span class="mono qa-res-name" :title="r.output || r.input">{{ baseName(r.output || r.input) }}</span>
          <span v-if="r.error" class="qa-res-err">{{ r.error }}</span>
          <button v-if="r.output" class="btn btn-sm btn-ghost" @click="revealItemInDir(r.output!)">{{ t("showInFolder") }}</button>
        </div>
        <div class="qa-f"><button class="btn btn-primary" @click="emit('close')">{{ t("close") }}</button></div>
      </div>
    </div>

    <OptionsModal
      v-if="optionsFor"
      :tool="optionsFor"
      :values="savedOptions(optionsFor)"
      @close="optionsFor = null"
      @saved="optionsFor = null; emit('config-changed')"
    />
  </div>
</template>

<style scoped>
.ov { position: fixed; inset: 0; background: rgba(15, 17, 23, .45); display: grid; place-items: center; z-index: 45; }
.qa { background: var(--panel); width: 520px; max-width: calc(100vw - 40px); max-height: calc(100vh - 60px); display: flex; flex-direction: column; }
.qa-h { display: flex; align-items: flex-start; justify-content: space-between; padding: 16px 18px 10px; }
.qa-h h3 { font-size: 16px; }
.qa-sub { margin: 2px 0 0; color: var(--muted); font-size: 12.5px; }
.qa-x { width: 28px; height: 28px; border: 0; border-radius: 8px; background: transparent; color: var(--muted); font-size: 20px; cursor: pointer; }
.qa-files { display: flex; flex-wrap: wrap; gap: 6px; padding: 0 18px 12px; }
.qa-list { overflow: auto; padding: 0 10px 12px; display: flex; flex-direction: column; gap: 4px; border-top: 1px solid var(--cardline); padding-top: 10px; }
.qa-none { color: var(--muted); padding: 10px 8px; }
.qa-item { display: flex; align-items: center; gap: 12px; text-align: left; padding: 10px 10px; border: 1px solid transparent; border-radius: 11px; background: transparent; cursor: pointer; }
.qa-item:hover { background: color-mix(in srgb, var(--acc) 8%, transparent); border-color: color-mix(in srgb, var(--acc) 30%, transparent); }
.qa-dot { width: 10px; height: 10px; border-radius: 50%; background: var(--acc); flex: none; }
.qa-txt { display: flex; flex-direction: column; min-width: 0; flex: 1; }
.qa-name { font-weight: 800; font-size: 13.5px; }
.qa-desc { font-size: 12px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.qa-gear { color: var(--faint); padding: 4px 6px; border-radius: 6px; }
.qa-gear:hover { background: var(--inset); color: var(--text); }
.qa-run { display: flex; align-items: center; gap: 12px; padding: 24px 18px; color: var(--muted); border-top: 1px solid var(--cardline); }
.qa-results { display: flex; flex-direction: column; gap: 8px; padding: 12px 18px 16px; border-top: 1px solid var(--cardline); overflow: auto; }
.qa-res { display: flex; align-items: center; gap: 10px; }
.qa-res-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12.5px; }
.qa-res-err { color: var(--bad); font-size: 12px; }
.qa-f { display: flex; justify-content: flex-end; margin-top: 6px; }
</style>

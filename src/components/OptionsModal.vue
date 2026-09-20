<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { t, bi } from "../i18n";
import type { ToolDefinition, ToolOption } from "../types";

const props = defineProps<{ tool: ToolDefinition; values: Record<string, unknown>; entryKey?: string | null }>();
const emit = defineEmits<{ (e: "close"): void; (e: "saved"): void }>();

// start from the schema defaults, then whatever the user saved
const form = ref<Record<string, unknown>>(
  Object.fromEntries(props.tool.options.map((o) => [o.name, props.values[o.name] ?? o.default])),
);
const visible = computed(() => props.tool.options.filter((o) => o.type !== "hidden"));
const busy = ref(false);
const error = ref("");

function invalid(): string {
  for (const o of visible.value) {
    const v = form.value[o.name];
    if (o.required && (v === "" || v == null)) return `${bi(o.label)}: ${t("required")}`;
    if (o.type === "text" && typeof v === "string") {
      if (o.min != null && v.length > 0 && v.length < o.min) return `${bi(o.label)}: ≥ ${o.min}`;
      if (o.max != null && v.length > o.max) return `${bi(o.label)}: ≤ ${o.max}`;
    }
    if (o.type === "number") {
      const n = Number(v);
      if (Number.isNaN(n)) return `${bi(o.label)}: ?`;
      if (o.min != null && n < o.min) return `${bi(o.label)}: ≥ ${o.min}`;
      if (o.max != null && n > o.max) return `${bi(o.label)}: ≤ ${o.max}`;
    }
  }
  return "";
}

async function save() {
  error.value = invalid();
  if (error.value) return;
  busy.value = true;
  try {
    const out: Record<string, unknown> = { ...form.value };
    for (const o of props.tool.options) {
      if (o.type === "number") out[o.name] = Number(out[o.name]);
      if (o.type === "text" && (out[o.name] === "" || out[o.name] == null) && !o.required) delete out[o.name];
    }
    await invoke("update_tool_options", { toolId: props.tool.id, options: out, key: props.entryKey ?? null });
    emit("saved");
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

function choiceKey(o: ToolOption, v: unknown) {
  return `${o.name}:${String(v)}`;
}
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") { e.stopPropagation(); emit("close"); }
}
onMounted(() => window.addEventListener("keydown", onKey, true));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey, true));
</script>

<template>
  <div class="ov" @click.self="emit('close')">
    <div class="md card" role="dialog" aria-modal="true">
      <div class="md-h">
        <h3>{{ t("optionsFor", { tool: bi(tool.name) }) }}</h3>
        <button class="md-x" :aria-label="t('close')" @click="emit('close')">×</button>
      </div>
      <div class="md-b">
        <p v-if="!visible.length" class="md-empty">{{ t("noOptions") }}</p>
        <div v-for="o in visible" :key="o.name" class="field">
          <template v-if="o.type === 'boolean'">
            <label class="md-row">
              <button type="button" class="switch" :class="{ on: !!form[o.name] }" :aria-pressed="!!form[o.name]" @click="form[o.name] = !form[o.name]"></button>
              <span>{{ bi(o.label) }}</span>
            </label>
          </template>
          <template v-else>
            <label :for="'opt-' + o.name">{{ bi(o.label) }}<span v-if="o.required" class="req"> *</span></label>
            <select v-if="o.type === 'select'" :id="'opt-' + o.name" v-model="form[o.name]" class="input">
              <option v-for="c in o.choices" :key="choiceKey(o, c.value)" :value="c.value">{{ bi(c.label) }}</option>
            </select>
            <input
              v-else-if="o.type === 'number'"
              :id="'opt-' + o.name"
              v-model="form[o.name]"
              class="input"
              type="number"
              :min="o.min ?? undefined"
              :max="o.max ?? undefined"
              :step="o.step ?? 1"
            />
            <input
              v-else
              :id="'opt-' + o.name"
              v-model="form[o.name]"
              class="input"
              :type="o.secret ? 'password' : 'text'"
              :maxlength="o.max ?? undefined"
              autocomplete="off"
            />
          </template>
        </div>
        <div v-if="error" class="md-err">{{ error }}</div>
      </div>
      <div class="md-f">
        <button class="btn" @click="emit('close')">{{ t("cancel") }}</button>
        <button class="btn btn-primary" :disabled="busy" @click="save">{{ t("save") }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.ov { position: fixed; inset: 0; background: rgba(15, 17, 23, .45); display: grid; place-items: center; z-index: 50; }
.md { background: var(--panel); width: 440px; max-width: calc(100vw - 40px); max-height: calc(100vh - 60px); display: flex; flex-direction: column; }
.md-h { display: flex; align-items: center; justify-content: space-between; padding: 16px 18px 12px; border-bottom: 1px solid var(--cardline); }
.md-h h3 { font-size: 15px; }
.md-x { width: 28px; height: 28px; border: 0; border-radius: 8px; background: transparent; color: var(--muted); font-size: 20px; cursor: pointer; }
.md-x:hover { background: var(--inset); color: var(--text); }
.md-b { padding: 16px 18px; display: flex; flex-direction: column; gap: 14px; overflow: auto; }
.md-empty { margin: 0; color: var(--muted); }
.md-row { display: flex; align-items: center; gap: 10px; cursor: pointer; text-transform: none; letter-spacing: 0; font-size: 13px; color: var(--text); font-weight: 600; }
.req { color: var(--bad); }
.md-err { background: var(--badSoft); color: var(--bad); border-radius: 9px; padding: 9px 11px; font-size: 12.5px; }
.md-f { display: flex; justify-content: flex-end; gap: 8px; padding: 12px 18px 16px; border-top: 1px solid var(--cardline); }
</style>

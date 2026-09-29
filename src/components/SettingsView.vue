<script setup lang="ts">
import { ref } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { t } from "../i18n";
import type { AppConfig, AuthState } from "../types";

const props = defineProps<{ config: AppConfig | null; auth: AuthState; version: string; siteBase: string }>();
const emit = defineEmits<{ (e: "save", cfg: AppConfig): void; (e: "sign-out"): void; (e: "check-updates"): Promise<boolean> | void }>();

const updateMsg = ref("");

function set<K extends keyof AppConfig["general"]>(key: K, value: AppConfig["general"][K]) {
  if (!props.config) return;
  emit("save", { ...props.config, general: { ...props.config.general, [key]: value } });
}
async function checkUpdates() {
  updateMsg.value = "…";
  const found = await (emit("check-updates") as unknown as Promise<boolean>);
  updateMsg.value = found ? "" : t("upToDate");
}
</script>

<template>
  <div class="sv" v-if="config">
    <section class="sv-sec card">
      <h3 class="sv-h">{{ t("general") }}</h3>
      <div class="sv-row">
        <div class="sv-lbl">{{ t("language") }}<span class="sv-desc">{{ t("languageDesc") }}</span></div>
        <div class="seg">
          <button class="seg-btn" :class="{ on: config.general.language === 'da' }" @click="set('language', 'da')">Dansk</button>
          <button class="seg-btn" :class="{ on: config.general.language === 'en' }" @click="set('language', 'en')">English</button>
        </div>
      </div>
      <div class="sv-row">
        <div class="sv-lbl">{{ t("theme") }}</div>
        <div class="seg">
          <button class="seg-btn" :class="{ on: config.general.theme === 'system' }" @click="set('theme', 'system')">{{ t("themeSystem") }}</button>
          <button class="seg-btn" :class="{ on: config.general.theme === 'light' }" @click="set('theme', 'light')">{{ t("themeLight") }}</button>
          <button class="seg-btn" :class="{ on: config.general.theme === 'dark' }" @click="set('theme', 'dark')">{{ t("themeDark") }}</button>
        </div>
      </div>
      <div class="sv-row">
        <div class="sv-lbl">{{ t("startOnLogin") }}<span class="sv-desc">{{ t("startOnLoginDesc") }}</span></div>
        <button class="switch" :class="{ on: config.general.startOnLogin }" :aria-pressed="config.general.startOnLogin" @click="set('startOnLogin', !config.general.startOnLogin)"></button>
      </div>
      <div class="sv-row">
        <div class="sv-lbl">{{ t("autoUpdate") }}<span class="sv-desc">{{ t("autoUpdateDesc") }}</span></div>
        <button class="switch" :class="{ on: config.general.autoUpdate !== false }" :aria-pressed="config.general.autoUpdate !== false" @click="set('autoUpdate', config.general.autoUpdate === false)"></button>
      </div>
      <div class="sv-row">
        <div class="sv-lbl">{{ t("notifications") }}<span class="sv-desc">{{ t("notificationsDesc") }}</span></div>
        <button class="switch" :class="{ on: config.general.showNotifications }" :aria-pressed="config.general.showNotifications" @click="set('showNotifications', !config.general.showNotifications)"></button>
      </div>
    </section>

    <section class="sv-sec card">
      <h3 class="sv-h">{{ t("account") }}</h3>
      <div class="sv-row">
        <div class="sv-lbl">{{ auth.user?.name || auth.user?.email }}<span class="sv-desc">{{ auth.user?.email }}</span></div>
        <span class="pill" :class="auth.isPro ? 'pill-brand' : 'pill-muted'">{{ (auth.plan || 'free').toUpperCase() }}</span>
      </div>
      <div class="sv-row">
        <div class="sv-lbl">{{ t("plan") }}</div>
        <div class="sv-btns">
          <button class="btn btn-sm" @click="openUrl(`${siteBase}/pricing`)">{{ t("managePlan") }}</button>
          <button class="btn btn-sm btn-ghost btn-danger" @click="emit('sign-out')">{{ t("signOut") }}</button>
        </div>
      </div>
    </section>

    <section class="sv-sec card">
      <h3 class="sv-h">{{ t("about") }}</h3>
      <div class="sv-row">
        <div class="sv-lbl">{{ t("version") }} <span class="mono">{{ version }}</span><span class="sv-desc">{{ t("processedVia") }}</span></div>
        <div class="sv-btns">
          <span v-if="updateMsg" class="sv-msg">{{ updateMsg }}</span>
          <button class="btn btn-sm" @click="checkUpdates">{{ t("checkUpdates") }}</button>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.sv { padding: 22px 24px 40px; max-width: 760px; margin: 0 auto; display: flex; flex-direction: column; gap: 14px; }
.sv-sec { padding: 6px 18px; }
.sv-h { font-size: 12px; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); padding: 12px 0 4px; }
.sv-row { display: flex; align-items: center; justify-content: space-between; gap: 18px; padding: 12px 0; border-top: 1px solid var(--cardline); }
.sv-lbl { font-weight: 700; display: flex; flex-direction: column; gap: 2px; }
.sv-desc { font-weight: 500; font-size: 12px; color: var(--muted); max-width: 440px; }
.sv-btns { display: flex; align-items: center; gap: 8px; }
.sv-msg { font-size: 12px; color: var(--muted); }
.seg { display: flex; gap: 2px; background: var(--inset); border: 1px solid var(--cardline); border-radius: 10px; padding: 3px; }
.seg-btn { height: 26px; padding: 0 10px; border: 0; border-radius: 7px; background: transparent; color: var(--muted); font-weight: 700; font-size: 12px; cursor: pointer; }
.seg-btn.on { background: var(--card); color: var(--text); box-shadow: var(--shadow); }
</style>

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { t } from "../i18n";
import type { AuthState } from "../types";

const props = defineProps<{ siteBase: string }>();
const emit = defineEmits<{ (e: "signed-in", auth: AuthState): void }>();

const email = ref("");
const password = ref("");
const remember = ref(false);
const error = ref("");
const busy = ref(false);

onMounted(async () => {
  try {
    const saved = await invoke<{ email: string; password: string } | null>("get_saved_credentials");
    if (saved) { email.value = saved.email; password.value = saved.password; remember.value = true; }
  } catch { /* none saved */ }
});

async function submit() {
  error.value = "";
  busy.value = true;
  try {
    const auth = await invoke<AuthState>("login", { email: email.value, password: password.value, remember: remember.value });
    emit("signed-in", auth);
  } catch (e: any) {
    error.value = String(e).replace(/^Error: /, "");
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="login">
    <div class="login-card card">
      <div class="login-brand">
        <span class="pdk-brand" style="font-size:34px">pdf<i class="pdk-dot"></i>dk</span>
        <span class="login-sub">Desktop</span>
      </div>
      <h1 class="login-h">{{ t("signInTitle") }}</h1>
      <p class="login-p">{{ t("signInSub") }}</p>

      <form class="login-form" @submit.prevent="submit">
        <div class="field">
          <label for="email">{{ t("email") }}</label>
          <input id="email" v-model="email" class="input" type="email" autocomplete="username" required :disabled="busy" />
        </div>
        <div class="field">
          <label for="password">{{ t("password") }}</label>
          <input id="password" v-model="password" class="input" type="password" autocomplete="current-password" required :disabled="busy" />
        </div>
        <label class="remember">
          <button type="button" class="switch" :class="{ on: remember }" :aria-pressed="remember" @click="remember = !remember"></button>
          <span>{{ t("rememberMe") }}</span>
        </label>
        <div v-if="error" class="login-err">{{ error }}</div>
        <button type="submit" class="btn btn-primary btn-lg" :disabled="busy">
          <span v-if="busy" class="spin" style="width:14px;height:14px;border-width:2px;border-top-color:#fff"></span>
          {{ busy ? t("signingIn") : t("signIn") }}
        </button>
      </form>

      <p class="login-note">{{ t("planNote") }}</p>
      <p class="login-links">
        <a href="#" @click.prevent="openUrl(`${props.siteBase}/register`)">{{ t("createAccount") }}</a>
        <span>·</span>
        <a href="#" @click.prevent="openUrl(`${props.siteBase}/forgot-password`)">{{ t("forgotPassword") }}</a>
      </p>
    </div>
  </div>
</template>

<style scoped>
.login { height: 100vh; display: grid; place-items: center; padding: 24px; background: radial-gradient(900px 500px at 50% -10%, var(--brandSoft), transparent 70%), var(--bg); }
.login-card { width: 380px; padding: 30px 30px 24px; }
.login-brand { display: flex; align-items: baseline; gap: 8px; margin-bottom: 18px; }
.login-sub { font-size: 12px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); }
.login-h { font-size: 19px; }
.login-p { margin: 4px 0 18px; color: var(--muted); }
.login-form { display: flex; flex-direction: column; gap: 14px; }
.remember { display: flex; align-items: center; gap: 10px; font-size: 12.5px; color: var(--text2); cursor: pointer; }
.login-err { background: var(--badSoft); color: var(--bad); border-radius: 9px; padding: 9px 11px; font-size: 12.5px; }
.login-note { margin: 18px 0 6px; font-size: 12px; color: var(--muted); text-align: center; }
.login-links { margin: 0; font-size: 12.5px; text-align: center; display: flex; justify-content: center; gap: 8px; color: var(--faint); }
</style>

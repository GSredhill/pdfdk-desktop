<script setup lang="ts">
import { ref, onMounted, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

// Types
interface User {
  id: number;
  email: string;
  name: string | null;
  subscriptionType: string | null;
}

interface AuthState {
  isAuthenticated: boolean;
  isPro: boolean;
  user: User | null;
  token: string | null;
  // Plan and usage limits
  plan: string | null;
  jobsLimit: number | null;
  jobsUsed: number | null;
  jobsRemaining: number | null;
  maxFileSizeMb: number | null;
  isUnlimited: boolean | null;
}

interface ToolDefinition {
  id: string;
  name: string;
  nameDa: string;
  description: string;
  descriptionDa: string;
  apiEndpoint: string;
  icon: string;
  hasOptions: boolean;
}

interface ToolConfig {
  id: string;
  enabled: boolean;
  folderPath: string | null;
  outputMode: string;
  options: Record<string, unknown>;
}

interface AppConfig {
  version: number;
  general: {
    startOnLogin: boolean;
    startMinimized: boolean;
    showNotifications: boolean;
    language: string;
  };
  tools: ToolConfig[];
}

// State
const loading = ref(true);
const authState = ref<AuthState>({
  isAuthenticated: false,
  isPro: false,
  user: null,
  token: null,
  plan: null,
  jobsLimit: null,
  jobsUsed: null,
  jobsRemaining: null,
  maxFileSizeMb: null,
  isUnlimited: null
});
const availableTools = ref<ToolDefinition[]>([]);
const config = ref<AppConfig | null>(null);

// Login form
const email = ref("");
const password = ref("");
const rememberMe = ref(false);
const loginError = ref("");
const loginLoading = ref(false);

// Current view
const currentView = ref<"login" | "main">("login");

// Active tab in main view
const activeTab = ref<"tools" | "website">("tools");

// Options modal
const showOptionsModal = ref(false);
const selectedTool = ref<ToolDefinition | null>(null);
const toolOptions = ref<Record<string, unknown>>({});

// Debug logs
const logs = ref<string[]>([]);
const showLogs = ref(false);
let logInterval: ReturnType<typeof setInterval> | null = null;

// Update state
const updateAvailable = ref<Update | null>(null);
const currentVersion = ref("");
const isUpdating = ref(false);

// Computed
const enabledTools = computed(() => {
  if (!config.value) return [];
  return config.value.tools.filter(t => t.enabled);
});

// Display plan name - show PRO for both 'pro' and 'team' plans
const displayPlan = computed(() => {
  const plan = authState.value.plan?.toLowerCase() || 'free';
  // Team and superadmin are essentially PRO+ for display
  if (plan === 'team' || plan === 'superadmin') {
    return 'PRO';
  }
  return plan.toUpperCase();
});

// CSS class for plan badge (team/pro use same styling)
const planClass = computed(() => {
  const plan = authState.value.plan?.toLowerCase() || 'free';
  if (plan === 'team' || plan === 'superadmin') {
    return 'pro';
  }
  return plan;
});

// Methods
async function checkAuth() {
  try {
    const result = await invoke<AuthState>("check_auth");
    authState.value = result;
    if (result.isAuthenticated) {
      currentView.value = "main";
      await loadConfig();
    }
  } catch (e) {
    console.error("Auth check failed:", e);
  }
}

async function login() {
  loginError.value = "";
  loginLoading.value = true;

  try {
    const result = await invoke<AuthState>("login", {
      email: email.value,
      password: password.value,
      remember: rememberMe.value
    });
    authState.value = result;
    currentView.value = "main";
    await loadConfig();
  } catch (e: any) {
    loginError.value = e.toString();
  } finally {
    loginLoading.value = false;
  }
}

async function loadSavedCredentials() {
  try {
    const saved = await invoke<{ email: string; password: string } | null>("get_saved_credentials");
    if (saved) {
      email.value = saved.email;
      password.value = saved.password;
      rememberMe.value = true;
    }
  } catch (e) {
    console.log("No saved credentials");
  }
}

async function logout() {
  try {
    await invoke("logout");
    authState.value = {
      isAuthenticated: false,
      isPro: false,
      user: null,
      token: null,
      plan: null,
      jobsLimit: null,
      jobsUsed: null,
      jobsRemaining: null,
      maxFileSizeMb: null,
      isUnlimited: null
    };
    currentView.value = "login";
    // Keep email/password if remember me was checked
    if (!rememberMe.value) {
      email.value = "";
      password.value = "";
    }
  } catch (e) {
    console.error("Logout failed:", e);
  }
}

async function loadConfig() {
  try {
    config.value = await invoke<AppConfig>("get_config");
    availableTools.value = await invoke<ToolDefinition[]>("get_available_tools");
    // Start watchers for any already-enabled tools
    await invoke("start_watchers");
    console.log("Watchers started for enabled tools");
  } catch (e) {
    console.error("Failed to load config:", e);
  }
}

async function selectFolder(toolId: string) {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: `Select folder for ${toolId}`,
    });

    if (selected && typeof selected === 'string') {
      await invoke("enable_tool", { toolId, folderPath: selected });
      await loadConfig();
    }
  } catch (e) {
    console.error("Failed to select folder:", e);
  }
}

async function disableTool(toolId: string) {
  try {
    await invoke("disable_tool", { toolId });
    await loadConfig();
  } catch (e) {
    console.error("Failed to disable tool:", e);
  }
}

function getToolConfig(toolId: string): ToolConfig | undefined {
  return config.value?.tools.find(t => t.id === toolId);
}

function isToolEnabled(toolId: string): boolean {
  const tc = getToolConfig(toolId);
  return tc?.enabled ?? false;
}

function getToolFolder(toolId: string): string | null {
  const tc = getToolConfig(toolId);
  return tc?.folderPath ?? null;
}

// SVG Icons as inline components matching pdf.dk website design
const svgIcons: Record<string, string> = {
  compress: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>`,
  text: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 7 4 4 20 4 20 7"/><line x1="9" y1="20" x2="15" y2="20"/><line x1="12" y1="4" x2="12" y2="20"/></svg>`,
  'file-word': `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><line x1="10" y1="9" x2="8" y2="9"/></svg>`,
  'file-excel': `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="3" y1="15" x2="21" y2="15"/><line x1="9" y1="3" x2="9" y2="21"/><line x1="15" y1="3" x2="15" y2="21"/></svg>`,
  image: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>`,
  rotate: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>`,
  unlock: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/></svg>`,
  scan: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="12" y1="18" x2="12" y2="12"/><line x1="9" y1="15" x2="15" y2="15"/></svg>`,
  expand: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 3 21 3 21 9"/><polyline points="9 21 3 21 3 15"/><line x1="21" y1="3" x2="14" y2="10"/><line x1="3" y1="21" x2="10" y2="14"/></svg>`,
  bookmark: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>`,
};

function getToolIconSvg(icon: string): string {
  return svgIcons[icon] || svgIcons['file-word'];
}

// Icon colors matching pdf.dk website
const iconColors: Record<string, string> = {
  compress: 'blue',
  text: 'green',       // Outline fonts
  'file-word': 'blue',
  'file-excel': 'green',
  image: 'green',      // PDF to JPG
  rotate: 'teal',
  unlock: 'pink',
  scan: 'violet',      // OCR
  expand: 'amber',     // Bleed
  bookmark: 'blue',
};

function getToolIconColor(icon: string): string {
  return iconColors[icon] || 'blue';
}

async function openWebsite() {
  try {
    await openUrl("https://pdf.dk");
  } catch (e) {
    console.error("Failed to open website:", e);
  }
}

async function openRegister() {
  try {
    await openUrl("https://pdf.dk/register");
  } catch (e) {
    console.error("Failed to open register page:", e);
  }
}

async function openForgotPassword() {
  try {
    await openUrl("https://pdf.dk/forgot-password");
  } catch (e) {
    console.error("Failed to open forgot password page:", e);
  }
}

function openOptions(tool: ToolDefinition) {
  selectedTool.value = tool;
  const tc = getToolConfig(tool.id);
  toolOptions.value = tc?.options ? { ...tc.options } : getDefaultOptions(tool.id);
  showOptionsModal.value = true;
}

function getDefaultOptions(toolId: string): Record<string, unknown> {
  switch (toolId) {
    case 'compress':
      return { quality: 'default' };  // Valid: low, default, high, maximum
    case 'rotate':
      return { degrees: 90 };
    case 'bleed':
      return { amount: 3 };
    case 'ocr':
      return { language: 'da' };
    default:
      return {};
  }
}

async function saveOptions() {
  if (!selectedTool.value) return;
  try {
    await invoke("update_tool_options", {
      toolId: selectedTool.value.id,
      options: toolOptions.value
    });
    await loadConfig();
    showOptionsModal.value = false;
  } catch (e) {
    console.error("Failed to save options:", e);
  }
}

function getToolOptions(toolId: string): Record<string, unknown> {
  const tc = getToolConfig(toolId);
  return tc?.options || getDefaultOptions(toolId);
}

// Debug logs functions
async function refreshLogs() {
  try {
    logs.value = await invoke<string[]>("get_logs");
  } catch (e) {
    console.error("Failed to get logs:", e);
  }
}

async function clearLogs() {
  try {
    await invoke("clear_logs");
    logs.value = [];
  } catch (e) {
    console.error("Failed to clear logs:", e);
  }
}

function toggleLogs() {
  showLogs.value = !showLogs.value;
  if (showLogs.value) {
    refreshLogs();
    logInterval = setInterval(refreshLogs, 1000);
  } else if (logInterval) {
    clearInterval(logInterval);
    logInterval = null;
  }
}

// Check for updates (but don't install - show button instead)
async function checkForUpdates() {
  try {
    currentVersion.value = await getVersion();

    // Try Tauri's built-in updater first
    try {
      const update = await check();
      if (update) {
        updateAvailable.value = update;
        console.log(`Update available via Tauri: ${update.version}`);
        return;
      }
    } catch (tauriError) {
      console.log("Tauri updater failed, trying manual check:", tauriError);
    }

    // Fallback: Manual version check via GitHub API
    try {
      const response = await fetch(
        "https://github.com/GSredhill/pdfdk-desktop/releases/latest/download/latest.json"
      );
      const data = await response.json();
      const latestVersion = data.version;

      if (latestVersion && latestVersion !== currentVersion.value) {
        // Create a fake Update object for the button
        updateAvailable.value = {
          version: latestVersion,
          downloadAndInstall: async () => {
            // This will fail and trigger the fallback
            throw new Error("Manual update - use direct download");
          }
        } as any;
        console.log(`Update available via manual check: ${latestVersion}`);
      }
    } catch (fetchError) {
      console.log("Manual update check also failed:", fetchError);
    }
  } catch (e: any) {
    console.error("Update check failed:", e);
  }
}

// Install update when user clicks the button
async function installUpdate() {
  if (!updateAvailable.value) return;

  isUpdating.value = true;
  try {
    await updateAvailable.value.downloadAndInstall();
    await relaunch();
  } catch (e) {
    console.error("Update install failed:", e);
    isUpdating.value = false;
    // Auto-install failed, download the installer directly
    await downloadInstallerDirectly();
  }
}

// Fallback: Download the DMG/EXE directly from GitHub
async function downloadInstallerDirectly() {
  try {
    const response = await fetch(
      "https://github.com/GSredhill/pdfdk-desktop/releases/latest/download/latest.json"
    );
    const data = await response.json();

    // Detect platform and get the right download URL
    const platform = navigator.platform.toLowerCase();
    let downloadUrl = "";

    if (platform.includes("mac")) {
      // Check if Apple Silicon or Intel based on userAgent
      const isArmMac = navigator.userAgent.includes("ARM") ||
                       (navigator as any).userAgentData?.platform === "macOS";

      if (isArmMac && data.platforms["darwin-aarch64"]) {
        downloadUrl = `https://github.com/GSredhill/pdfdk-desktop/releases/download/v${data.version}/PDF.dk.Desktop_${data.version}_aarch64.dmg`;
      } else if (data.platforms["darwin-x86_64"]) {
        downloadUrl = `https://github.com/GSredhill/pdfdk-desktop/releases/download/v${data.version}/PDF.dk.Desktop_${data.version}_x64.dmg`;
      }
    } else if (platform.includes("win")) {
      // Windows - use the MSI installer
      downloadUrl = `https://github.com/GSredhill/pdfdk-desktop/releases/download/v${data.version}/PDF.dk.Desktop_${data.version}_x64_en-US.msi`;
    }

    if (downloadUrl) {
      await openUrl(downloadUrl);
    } else {
      // Fallback to download page if platform not detected
      await openUrl("https://pdf.dk/desktop");
    }
  } catch (e) {
    console.error("Failed to get download URL:", e);
    await openUrl("https://pdf.dk/desktop");
  }
}

// Initialize
onMounted(async () => {
  // Check for updates in background
  checkForUpdates();

  await checkAuth();
  if (currentView.value === "login") {
    await loadSavedCredentials();
  }
  loading.value = false;
});
</script>

<template>
  <div class="app">
    <!-- Loading -->
    <div v-if="loading" class="loading">
      <div class="spinner"></div>
      <p>Loading...</p>
    </div>

    <!-- Login View -->
    <div v-else-if="currentView === 'login'" class="login-view">
      <div class="login-card">
        <div class="logo">
          <img src="/logo.svg" alt="PDF.dk" class="logo-img" />
          <h1>PDF.dk Desktop</h1>
        </div>

        <p class="subtitle">Sign in to your account</p>

        <form @submit.prevent="login" class="login-form">
          <div class="form-group">
            <label for="email">Email</label>
            <input
              id="email"
              v-model="email"
              type="email"
              placeholder="your@email.com"
              required
              :disabled="loginLoading"
            />
          </div>

          <div class="form-group">
            <label for="password">Password</label>
            <input
              id="password"
              v-model="password"
              type="password"
              placeholder="••••••••"
              required
              :disabled="loginLoading"
            />
          </div>

          <div class="form-group-checkbox">
            <input
              id="remember"
              v-model="rememberMe"
              type="checkbox"
              :disabled="loginLoading"
            />
            <label for="remember">Remember me</label>
          </div>

          <div v-if="loginError" class="error">
            {{ loginError }}
          </div>

          <button type="submit" class="btn-primary" :disabled="loginLoading">
            {{ loginLoading ? 'Signing in...' : 'Sign In' }}
          </button>
        </form>

        <p class="pro-note">
          Free: 20 jobs/month • PRO: Unlimited<br>
          <a href="#" @click.prevent="openRegister">Create an account</a> • <a href="#" @click.prevent="openForgotPassword">Forgot password?</a>
        </p>
      </div>
    </div>

    <!-- Main View -->
    <div v-else class="main-view">
      <header class="header">
        <div class="header-left">
          <h1>PDF.dk Desktop</h1>
        </div>
        <div class="header-right">
          <button
            v-if="updateAvailable"
            @click="installUpdate"
            class="btn-update"
            :disabled="isUpdating"
          >
            {{ isUpdating ? 'Updating...' : `Update to v${updateAvailable.version}` }}
          </button>
          <span class="user-email">{{ authState.user?.email }}</span>
          <span class="plan-badge" :class="planClass">{{ displayPlan }}</span>
          <span v-if="authState.isUnlimited" class="usage-text">Unlimited</span>
          <span v-else-if="authState.jobsLimit" class="usage-text">{{ authState.jobsUsed || 0 }}/{{ authState.jobsLimit }} jobs</span>
          <button @click="logout" class="btn-text">Sign Out</button>
        </div>
      </header>

      <!-- Tab Navigation -->
      <div class="tab-nav">
        <button
          @click="activeTab = 'tools'"
          class="tab-btn"
          :class="{ active: activeTab === 'tools' }"
        >
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" class="tab-icon"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
          Watched Folders
        </button>
        <button
          @click="openWebsite"
          class="tab-btn"
        >
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" class="tab-icon"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>
          Open Website
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" class="external-icon"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
        </button>
      </div>

      <!-- Tools Tab -->
      <main v-if="activeTab === 'tools'" class="content">
        <section class="tools-section">
          <h2>Watched Folders</h2>
          <p class="section-desc">
            Drop PDF files into these folders to automatically process them.
          </p>

          <div class="tools-grid">
            <div
              v-for="tool in availableTools"
              :key="tool.id"
              class="tool-card"
              :class="{ enabled: isToolEnabled(tool.id) }"
            >
              <div class="tool-header">
                <div class="tool-icon" :class="getToolIconColor(tool.icon)" v-html="getToolIconSvg(tool.icon)"></div>
                <div class="tool-info">
                  <h3>{{ tool.name }}</h3>
                  <p>{{ tool.description }}</p>
                </div>
                <button
                  v-if="tool.hasOptions && isToolEnabled(tool.id)"
                  @click="openOptions(tool)"
                  class="btn-options"
                  title="Options"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
                </button>
              </div>

              <div v-if="isToolEnabled(tool.id)" class="tool-config">
                <div class="folder-path">
                  <svg class="folder-icon" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
                  <span class="path">{{ getToolFolder(tool.id) }}</span>
                </div>
                <!-- Show current options if applicable -->
                <div v-if="tool.hasOptions" class="tool-options-summary">
                  <span v-if="tool.id === 'rotate'">
                    Rotation: {{ getToolOptions(tool.id).degrees }}°
                  </span>
                  <span v-if="tool.id === 'compress'">
                    Quality: {{ getToolOptions(tool.id).quality || 'default' }}
                  </span>
                  <span v-if="tool.id === 'bleed'">
                    Bleed: {{ getToolOptions(tool.id).amount }}mm
                  </span>
                  <span v-if="tool.id === 'ocr'">
                    Language: {{ getToolOptions(tool.id).language === 'da' ? 'Danish' : 'English' }}
                  </span>
                </div>
                <div class="tool-actions">
                  <button @click="selectFolder(tool.id)" class="btn-small">
                    Change Folder
                  </button>
                  <button @click="disableTool(tool.id)" class="btn-small btn-danger">
                    Disable
                  </button>
                </div>
              </div>

              <div v-else class="tool-enable">
                <button @click="selectFolder(tool.id)" class="btn-primary">
                  Enable & Select Folder
                </button>
              </div>
            </div>
          </div>
        </section>

        <section class="status-section">
          <h2>Status</h2>
          <div class="status-card">
            <div class="status-item">
              <svg class="status-icon" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
              <span>Watching {{ enabledTools.length }} folder(s)</span>
            </div>
            <div class="status-item">
              <svg class="status-icon" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>
              <span>Ready to process files</span>
            </div>
          </div>
        </section>
      </main>


      <!-- Options Modal -->
      <div v-if="showOptionsModal" class="modal-overlay" @click.self="showOptionsModal = false">
        <div class="modal">
          <div class="modal-header">
            <h3>{{ selectedTool?.name }} Options</h3>
            <button @click="showOptionsModal = false" class="modal-close">×</button>
          </div>
          <div class="modal-body">
            <!-- Rotate options -->
            <div v-if="selectedTool?.id === 'rotate'" class="form-group">
              <label>Rotation:</label>
              <select v-model="toolOptions.degrees">
                <option :value="90">90° clockwise</option>
                <option :value="180">180°</option>
                <option :value="270">270° counter-clockwise</option>
              </select>
            </div>

            <!-- Compress options -->
            <div v-if="selectedTool?.id === 'compress'" class="form-group">
              <label>Quality:</label>
              <select v-model="toolOptions.quality">
                <option value="low">Low (smallest file)</option>
                <option value="default">Default (balanced)</option>
                <option value="high">High (better quality)</option>
                <option value="maximum">Maximum (best quality)</option>
              </select>
            </div>

            <!-- Bleed options -->
            <div v-if="selectedTool?.id === 'bleed'" class="form-group">
              <label>Bleed amount:</label>
              <select v-model="toolOptions.amount">
                <option :value="3">3mm</option>
                <option :value="5">5mm</option>
                <option :value="10">10mm</option>
              </select>
            </div>

            <!-- OCR options -->
            <div v-if="selectedTool?.id === 'ocr'" class="form-group">
              <label>Language:</label>
              <select v-model="toolOptions.language">
                <option value="da">Danish</option>
                <option value="en">English</option>
              </select>
            </div>
          </div>
          <div class="modal-footer">
            <button @click="showOptionsModal = false" class="btn-small">Cancel</button>
            <button @click="saveOptions" class="btn-primary">Save</button>
          </div>
        </div>
      </div>

      <footer class="footer">
        <p>PDF.dk Desktop v{{ currentVersion || '...' }} • Files are processed via pdf.dk API • <a href="#" @click.prevent="toggleLogs" class="footer-link">{{ showLogs ? 'Hide Logs' : 'Logs' }}</a></p>
      </footer>

      <!-- Debug Logs Panel -->
      <div v-if="showLogs" class="logs-panel">
        <div class="logs-header">
          <h3>Debug Logs</h3>
          <div class="logs-actions">
            <button @click="refreshLogs" class="btn-small">Refresh</button>
            <button @click="clearLogs" class="btn-small">Clear</button>
            <button @click="showLogs = false" class="btn-small">Close</button>
          </div>
        </div>
        <div class="logs-content">
          <div v-for="(log, index) in logs" :key="index" class="log-entry">
            {{ log }}
          </div>
          <div v-if="logs.length === 0" class="no-logs">
            No logs yet. Drop a file into a watched folder to see activity.
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

:root {
  --primary: #3b82f6;
  --primary-dark: #2563eb;
  --danger: #ef4444;
  --success: #22c55e;
  --bg: #f8fafc;
  --bg-card: #ffffff;
  --text: #1e293b;
  --text-muted: #64748b;
  --border: #e2e8f0;
  --radius: 12px;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  background: var(--bg);
  color: var(--text);
  line-height: 1.5;
}

.app {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

/* Loading */
.loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100vh;
  gap: 1rem;
}

.spinner {
  width: 40px;
  height: 40px;
  border: 3px solid var(--border);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* Login View */
.login-view {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  padding: 2rem;
}

.login-card {
  background: var(--bg-card);
  border-radius: var(--radius);
  padding: 2.5rem;
  width: 100%;
  max-width: 400px;
  box-shadow: 0 4px 6px -1px rgba(0,0,0,0.1), 0 2px 4px -1px rgba(0,0,0,0.06);
}

.logo {
  text-align: center;
  margin-bottom: 1.5rem;
}

.logo-img {
  width: 64px;
  height: 64px;
  margin-bottom: 0.5rem;
}

.logo h1 {
  font-size: 1.5rem;
  font-weight: 700;
  color: var(--text);
}

.subtitle {
  text-align: center;
  color: var(--text-muted);
  margin-bottom: 1.5rem;
}

.login-form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.form-group label {
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--text);
}

.form-group input {
  padding: 0.75rem 1rem;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 1rem;
  transition: border-color 0.2s;
}

.form-group input:focus {
  outline: none;
  border-color: var(--primary);
}

.form-group-checkbox {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.form-group-checkbox input[type="checkbox"] {
  width: 1rem;
  height: 1rem;
  cursor: pointer;
}

.form-group-checkbox label {
  font-size: 0.875rem;
  color: var(--text-muted);
  cursor: pointer;
}

.error {
  background: #fef2f2;
  border: 1px solid #fecaca;
  color: var(--danger);
  padding: 0.75rem 1rem;
  border-radius: 8px;
  font-size: 0.875rem;
}

.btn-primary {
  background: var(--primary);
  color: white;
  border: none;
  padding: 0.75rem 1.5rem;
  border-radius: 8px;
  font-size: 1rem;
  font-weight: 500;
  cursor: pointer;
  transition: background 0.2s;
}

.btn-primary:hover {
  background: var(--primary-dark);
}

.btn-primary:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.pro-note {
  text-align: center;
  margin-top: 1.5rem;
  font-size: 0.875rem;
  color: var(--text-muted);
}

.pro-note a {
  color: var(--primary);
  text-decoration: none;
}

.pro-note a:hover {
  text-decoration: underline;
}

/* Main View */
.main-view {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem 1.5rem;
  background: var(--bg-card);
  border-bottom: 1px solid var(--border);
}

.header h1 {
  font-size: 1.25rem;
  font-weight: 600;
}

.header-right {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.user-email {
  font-size: 0.875rem;
  color: var(--text-muted);
}

/* Plan badges */
.plan-badge {
  font-size: 0.75rem;
  font-weight: 600;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  text-transform: uppercase;
}

.plan-badge.pro, .plan-badge.team {
  background: var(--primary);
  color: white;
}

.plan-badge.free {
  background: #22c55e;
  color: white;
}

.plan-badge.guest {
  background: #64748b;
  color: white;
}

.usage-text {
  font-size: 0.75rem;
  color: var(--text-muted);
  padding: 0.25rem 0.5rem;
  background: var(--bg);
  border-radius: 4px;
}

.btn-text {
  background: none;
  border: none;
  color: var(--text-muted);
  font-size: 0.875rem;
  cursor: pointer;
}

.btn-text:hover {
  color: var(--text);
}

/* Update button */
.btn-update {
  background: #22c55e;
  color: white;
  border: none;
  padding: 0.5rem 1rem;
  border-radius: 6px;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  animation: pulse 2s infinite;
}

.btn-update:hover {
  background: #16a34a;
}

.btn-update:disabled {
  opacity: 0.7;
  cursor: wait;
  animation: none;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.8; }
}

.content {
  flex: 1;
  padding: 1.5rem;
  max-width: 800px;
  margin: 0 auto;
  width: 100%;
}

.tools-section, .status-section {
  margin-bottom: 2rem;
}

.tools-section h2, .status-section h2 {
  font-size: 1.125rem;
  font-weight: 600;
  margin-bottom: 0.5rem;
}

.section-desc {
  color: var(--text-muted);
  font-size: 0.875rem;
  margin-bottom: 1rem;
}

.tools-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 1rem;
}

.tool-card {
  background: var(--bg-card);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 1.25rem;
}

.tool-card.enabled {
  border-color: var(--primary);
  box-shadow: 0 0 0 1px var(--primary);
}

.tool-header {
  display: flex;
  gap: 1rem;
  margin-bottom: 1rem;
}

.tool-icon {
  width: 48px;
  height: 48px;
  min-width: 48px;
  border-radius: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.tool-icon svg {
  width: 24px;
  height: 24px;
}

/* Color variants matching pdf.dk website */
.tool-icon.blue { background: #EFF6FF; color: #3B82F6; }
.tool-icon.green { background: #F0FDF4; color: #22C55E; }
.tool-icon.teal { background: #F0FDFA; color: #14B8A6; }
.tool-icon.pink { background: #FDF2F8; color: #EC4899; }
.tool-icon.violet { background: #F5F3FF; color: #8B5CF6; }
.tool-icon.amber { background: #FFFBEB; color: #F59E0B; }

.tool-info h3 {
  font-size: 1rem;
  font-weight: 600;
  margin-bottom: 0.25rem;
}

.tool-info p {
  font-size: 0.875rem;
  color: var(--text-muted);
}

.tool-config {
  background: var(--bg);
  border-radius: 8px;
  padding: 1rem;
}

.folder-path {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.75rem;
  font-size: 0.875rem;
}

.folder-icon {
  width: 16px;
  height: 16px;
  color: var(--text-muted);
  flex-shrink: 0;
}

.path {
  color: var(--text-muted);
  word-break: break-all;
}

.tool-actions {
  display: flex;
  gap: 0.5rem;
}

.btn-small {
  background: var(--bg-card);
  border: 1px solid var(--border);
  padding: 0.5rem 0.75rem;
  border-radius: 6px;
  font-size: 0.75rem;
  cursor: pointer;
}

.btn-small:hover {
  background: var(--bg);
}

.btn-danger {
  color: var(--danger);
  border-color: var(--danger);
}

.btn-danger:hover {
  background: #fef2f2;
}

.tool-enable {
  display: flex;
  justify-content: center;
}

.status-card {
  background: var(--bg-card);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 1rem;
}

.status-item {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.5rem 0;
  font-size: 0.875rem;
}

.status-icon {
  width: 18px;
  height: 18px;
  color: var(--success);
  flex-shrink: 0;
}

.footer {
  padding: 1rem 1.5rem;
  text-align: center;
  font-size: 0.75rem;
  color: var(--text-muted);
  border-top: 1px solid var(--border);
}

/* Tab Navigation */
.tab-nav {
  display: flex;
  gap: 0;
  background: var(--bg-card);
  border-bottom: 1px solid var(--border);
  padding: 0 1rem;
}

.tab-btn {
  background: none;
  border: none;
  padding: 1rem 1.5rem;
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--text-muted);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  transition: all 0.2s;
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.tab-btn:hover {
  color: var(--text);
}

.tab-btn.active {
  color: var(--primary);
  border-bottom-color: var(--primary);
}

.tab-icon {
  width: 18px;
  height: 18px;
}

.external-icon {
  width: 14px;
  height: 14px;
  opacity: 0.5;
}

/* Options Button */
.btn-options {
  background: none;
  border: none;
  cursor: pointer;
  padding: 0.25rem;
  opacity: 0.5;
  transition: opacity 0.2s;
  color: var(--text-muted);
}

.btn-options svg {
  width: 18px;
  height: 18px;
}

.btn-options:hover {
  opacity: 1;
  color: var(--text);
}

/* Tool Options Summary */
.tool-options-summary {
  font-size: 0.75rem;
  color: var(--text-muted);
  margin-bottom: 0.5rem;
  padding: 0.25rem 0.5rem;
  background: var(--bg-card);
  border-radius: 4px;
  display: inline-block;
}

/* Modal */
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.modal {
  background: var(--bg-card);
  border-radius: var(--radius);
  width: 100%;
  max-width: 400px;
  box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1);
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem 1.5rem;
  border-bottom: 1px solid var(--border);
}

.modal-header h3 {
  font-size: 1.125rem;
  font-weight: 600;
}

.modal-close {
  background: none;
  border: none;
  font-size: 1.5rem;
  cursor: pointer;
  color: var(--text-muted);
  line-height: 1;
}

.modal-close:hover {
  color: var(--text);
}

.modal-body {
  padding: 1.5rem;
}

.modal-body .form-group {
  margin-bottom: 1rem;
}

.modal-body .form-group:last-child {
  margin-bottom: 0;
}

.modal-body label {
  display: block;
  margin-bottom: 0.5rem;
  font-weight: 500;
  font-size: 0.875rem;
}

.modal-body select {
  width: 100%;
  padding: 0.75rem 1rem;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 1rem;
  background: var(--bg-card);
}

.modal-body select:focus {
  outline: none;
  border-color: var(--primary);
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
  padding: 1rem 1.5rem;
  border-top: 1px solid var(--border);
}

.tool-header {
  display: flex;
  gap: 1rem;
  margin-bottom: 1rem;
  align-items: flex-start;
}

/* Footer link for logs */
.footer-link {
  color: var(--text-muted);
  text-decoration: none;
  transition: color 0.2s;
}

.footer-link:hover {
  color: var(--primary);
  text-decoration: underline;
}

.logs-panel {
  position: fixed;
  bottom: 0;
  left: 0;
  right: 0;
  height: 280px;
  background: #1e293b;
  border-top: 2px solid var(--primary);
  display: flex;
  flex-direction: column;
  z-index: 1000;
}

.logs-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 16px;
  background: #0f172a;
  border-bottom: 1px solid #334155;
}

.logs-header h3 {
  color: #e2e8f0;
  font-size: 0.875rem;
  font-weight: 600;
}

.logs-actions {
  display: flex;
  gap: 8px;
}

.logs-actions .btn-small {
  background: #334155;
  border: 1px solid #475569;
  color: #e2e8f0;
  padding: 4px 10px;
  font-size: 0.75rem;
}

.logs-actions .btn-small:hover {
  background: #475569;
}

.logs-content {
  flex: 1;
  overflow-y: auto;
  padding: 12px 16px;
  font-family: 'Monaco', 'Menlo', 'Ubuntu Mono', monospace;
  font-size: 12px;
  line-height: 1.6;
}

.log-entry {
  padding: 2px 0;
  color: #22c55e;
  white-space: pre-wrap;
  word-break: break-all;
}

.log-entry:nth-child(odd) {
  color: #4ade80;
}

.no-logs {
  color: #64748b;
  font-style: italic;
  text-align: center;
  padding: 2rem;
}

/* Adjust content when logs are open */
.main-view:has(.logs-panel) .content {
  padding-bottom: 300px;
}

.main-view:has(.logs-panel) .footer {
  margin-bottom: 280px;
}
</style>

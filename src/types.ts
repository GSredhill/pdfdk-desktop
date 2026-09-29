// Shapes shared with the Rust side. Catalogue types are snake_case (the
// API's JSON, passed through untouched); app state is camelCase.

export interface Bilingual {
  da: string;
  en: string;
}

export interface OptionChoice {
  value: string | number | boolean;
  label: Bilingual;
}

export interface ToolOption {
  name: string;
  type: "select" | "number" | "boolean" | "text" | "hidden";
  label?: Bilingual | null;
  default: unknown;
  choices: OptionChoice[];
  min?: number | null;
  max?: number | null;
  step?: number | null;
  required: boolean;
  secret: boolean;
}

export interface ToolDefinition {
  id: string;
  endpoint: string;
  file_field: string;
  accepts: string[];
  output: string;
  category: string;
  category_label: Bilingual;
  name: Bilingual;
  description: Bilingual;
  web_path: Bilingual;
  options: ToolOption[];
}

export interface ToolConfig {
  id: string;
  key: string; // one entry per watched folder; a tool can have several
  enabled: boolean;
  folderPath: string | null;
  outputMode: string | { custom: string }; // "subfolder" | "same-folder" | { custom: path }
  options: Record<string, unknown>;
  endpoint?: string | null;
  fileField?: string | null;
  accepts?: string[];
  output?: string | null;
  name?: Bilingual | null;
}

/** a desktop widget: one tool with its own options, sitting on the desktop (0.4.0) */
export interface WidgetConfig {
  id: string;
  toolId: string;
  options: Record<string, unknown>;
  combine: boolean;
  x: number | null;
  y: number | null;
}

export interface AppConfig {
  version: number;
  general: {
    startOnLogin: boolean;
    startMinimized: boolean;
    showNotifications: boolean;
    autoUpdate?: boolean;
    language: string;
    theme: string;
  };
  tools: ToolConfig[];
  widgets?: WidgetConfig[];
  auth?: unknown;
}

export interface User {
  id: number;
  email: string;
  name: string | null;
  isSuperadmin: boolean;
  role: string | null;
  avatarUrl?: string | null;
}

export interface AuthState {
  isAuthenticated: boolean;
  isPro: boolean;
  user: User | null;
  token: string | null;
  plan: string | null;
  jobsLimit: number | null;
  jobsUsed: number | null;
  jobsRemaining: number | null;
  maxFileSizeMb: number | null;
  isUnlimited: boolean | null;
}

export interface Job {
  id: string;
  toolId: string;
  toolName: string;
  inputFile: string;
  outputFile: string | null;
  status: "queued" | "processing" | "completed" | "failed";
  error: string | null;
  createdAt: number;
  completedAt: number | null;
}

export interface FileResult {
  input: string;
  output: string | null;
  error: string | null;
}

export const CATEGORY_ORDER = ["Optimér", "Organisér", "Konvertér", "Redigér", "Sikkerhed", "Tryk"];

export const CATEGORY_VAR: Record<string, string> = {
  "Optimér": "--cat-optimer",
  "Organisér": "--cat-organiser",
  "Konvertér": "--cat-konverter",
  "Redigér": "--cat-rediger",
  "Sikkerhed": "--cat-sikkerhed",
  "Tryk": "--cat-tryk",
};

export function emptyAuth(): AuthState {
  return {
    isAuthenticated: false, isPro: false, user: null, token: null, plan: null,
    jobsLimit: null, jobsUsed: null, jobsRemaining: null, maxFileSizeMb: null, isUnlimited: null,
  };
}

export function fileExt(path: string): string {
  const m = /\.([a-z0-9]+)$/i.exec(path);
  return m ? m[1].toLowerCase() : "";
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

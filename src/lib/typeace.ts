export type AiStyle = "casual" | "professional" | "creative";
export type InferenceMode = "local" | "cloud";
export type OutputLanguage =
  | "auto"
  | "zh"
  | "en"
  | "ja"
  | "ko"
  | "es"
  | "fr"
  | "de";
export type HotkeyMode = "tab" | "ctrlSpace" | "custom";

export interface Settings {
  enabled: boolean;
  triggerDelayMs: number;
  aiStyle: AiStyle;
  inferenceMode: InferenceMode;
  outputLanguage: OutputLanguage;
  requestOnBoundaryOnly: boolean;
  localBaseUrl: string;
  localModelZh: string;
  localModelEn: string;
  localMaxTokens: number;
  cloudBaseUrl: string;
  cloudApiKey: string;
  cloudModel: string;
  cloudMaxTokens: number;
  hotkeyMode: HotkeyMode;
  customHotkey: string;
  autostart: boolean;
  minimumLength: number;
}

export interface UsageStats {
  dayKey: string;
  usedToday: number;
  freeLimit: number;
}

export interface AppSnapshot {
  settings: Settings;
  usage: UsageStats;
  suggestion: string | null;
  ghostVisible: boolean;
  remainingToday: number | null;
}

export interface GhostEventPayload {
  text: string;
  visible: boolean;
}

export const defaultSettings: Settings = {
  enabled: true,
  triggerDelayMs: 90,
  aiStyle: "casual",
  inferenceMode: "cloud",
  outputLanguage: "zh",
  requestOnBoundaryOnly: false,
  localBaseUrl: "http://127.0.0.1:11434/api/generate",
  localModelZh: "qwen2.5:1.5b",
  localModelEn: "llama3.2:1b",
  localMaxTokens: 32,
  cloudBaseUrl: "https://api.deepseek.com/v1/chat/completions",
  cloudApiKey: "",
  cloudModel: "deepseek-chat",
  cloudMaxTokens: 32,
  hotkeyMode: "tab",
  customHotkey: "Ctrl+Shift+Space",
  autostart: false,
  minimumLength: 1,
};

export const defaultUsage: UsageStats = {
  dayKey: new Date().toISOString().slice(0, 10),
  usedToday: 0,
  freeLimit: 50,
};

export const defaultSnapshot: AppSnapshot = {
  settings: defaultSettings,
  usage: defaultUsage,
  suggestion: null,
  ghostVisible: false,
  remainingToday: null,
};

export type AiStyle = "casual" | "professional" | "creative";
export type HotkeyMode = "tab" | "ctrlSpace" | "custom";

export interface Settings {
  enabled: boolean;
  triggerDelayMs: number;
  aiStyle: AiStyle;
  hotkeyMode: HotkeyMode;
  customHotkey: string;
  isPro: boolean;
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
  triggerDelayMs: 500,
  aiStyle: "casual",
  hotkeyMode: "tab",
  customHotkey: "Ctrl+Shift+Space",
  isPro: false,
  autostart: false,
  minimumLength: 10,
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
  remainingToday: 50,
};

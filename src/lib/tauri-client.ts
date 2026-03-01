import {
  AppSnapshot,
  defaultSnapshot,
  GhostEventPayload,
  Settings,
} from "@/lib/typeace";

type Unlisten = () => void;

export const isTauriRuntime = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  if (!isTauriRuntime()) {
    throw new Error("Tauri runtime unavailable");
  }
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export async function fetchAppState(): Promise<AppSnapshot> {
  if (!isTauriRuntime()) {
    return defaultSnapshot;
  }
  return invoke<AppSnapshot>("get_app_state");
}

export async function updateAppSettings(settings: Settings): Promise<AppSnapshot> {
  return invoke<AppSnapshot>("update_settings", { settings });
}

export async function acceptSuggestion(): Promise<boolean> {
  return invoke<boolean>("accept_suggestion");
}

export async function dismissGhost(): Promise<void> {
  return invoke<void>("dismiss_ghost");
}

export async function clearUsage(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>("clear_usage");
}

export async function listenState(
  callback: (payload: AppSnapshot) => void,
): Promise<Unlisten> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<AppSnapshot>("typeace://state", (event) => {
    callback(event.payload);
  });
  return unlisten;
}

export async function listenErrors(
  callback: (message: string) => void,
): Promise<Unlisten> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<{ message: string }>("typeace://error", (event) =>
    callback(event.payload.message),
  );
  return unlisten;
}

export async function listenGhostPayload(
  callback: (payload: GhostEventPayload) => void,
): Promise<Unlisten> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<GhostEventPayload>("typeace://ghost", (event) => {
    callback(event.payload);
  });
  return unlisten;
}

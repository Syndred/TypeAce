
use std::{
  collections::{HashSet, VecDeque},
  fs,
  path::PathBuf,
  sync::{Arc, Mutex, OnceLock},
  time::{Duration, Instant},
};

use arboard::Clipboard;
use chrono::Local;
use reqwest::Client;
use rdev::{listen, simulate, Event, EventType, Key};
use serde::{Deserialize, Serialize};
use tauri::{
  menu::{Menu, MenuItem},
  tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
  AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size, State, WindowEvent, Wry,
};
use tokio::sync::mpsc;

const FREE_DAILY_LIMIT: u32 = 50;
const DEFAULT_MIN_TRIGGER_LEN: usize = 1;
const LEGACY_DEFAULT_MIN_TRIGGER_LEN: usize = 10;
const MAX_CONTEXT_CHARS: usize = 300;
const MAX_HISTORY_ITEMS: usize = 6;
const MAX_HISTORY_CONTEXT_CHARS: usize = 480;
const MAX_SESSION_CONTEXT_CHARS: usize = 6000;
const MAX_SENTENCE_TAIL_CHARS: usize = 180;
const DEFAULT_LOCAL_OLLAMA_URL: &str = "http://127.0.0.1:11434/api/generate";
const DEFAULT_LOCAL_MODEL_ZH: &str = "qwen2.5:1.5b";
const DEFAULT_LOCAL_MODEL_EN: &str = "llama3.2:1b";
const DEFAULT_LOCAL_MAX_TOKENS: u16 = 32;
const DEFAULT_CLOUD_CHAT_URL: &str = "https://api.deepseek.com/v1/chat/completions";
const DEFAULT_CLOUD_MODEL: &str = "deepseek-chat";
const DEFAULT_CLOUD_MAX_TOKENS: u16 = 32;
const RELAX_COMPLETION_GUARDS: bool = true;
const MVP_LOW_LATENCY_MODE: bool = true;
const MVP_FORCE_CLOUD: bool = true;
const MVP_EMBEDDED_CLOUD_API_KEY: &str = "sk-8f8c1464c1aa492c96ecc68a0542b783";
const MVP_TRIGGER_DELAY_MIN_MS: u64 = 60;
const MVP_TRIGGER_DELAY_MAX_MS: u64 = 500;
const MVP_MAX_PROMPT_CHARS: usize = 420;
const MVP_MAX_HISTORY_CHARS: usize = 140;
const MVP_MAX_OUTPUT_TOKENS: u16 = 48;
const MVP_REQUEST_TIMEOUT_SECS: u64 = 8;
const MVP_CLOUD_TIMEOUT_SECS: u64 = 6;
const MVP_KEEP_ALIVE: &str = "30m";
const LATENCY_METRICS_WINDOW_SIZE: usize = 200;
const LATENCY_METRICS_LOG_EVERY: u64 = 20;
const STATE_FILENAME: &str = "typeace-state.json";
const GHOST_MIN_WIDTH: i32 = 140;
const GHOST_MAX_WIDTH: i32 = 720;
const GHOST_HEIGHT: i32 = 34;
const GHOST_OFFSET_X: i32 = 1;
const GHOST_OFFSET_Y: i32 = -2;

static LAST_GHOST_ANCHOR: OnceLock<Mutex<Option<(i32, i32)>>> = OnceLock::new();
static PREDICTION_LATENCY_METRICS: OnceLock<Mutex<LatencyMetricsStore>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
enum AiStyle {
  #[default]
  Casual,
  Professional,
  Creative,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
enum HotkeyMode {
  #[default]
  Tab,
  CtrlSpace,
  Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum InferenceMode {
  #[default]
  Local,
  Cloud,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum OutputLanguage {
  #[default]
  Auto,
  Zh,
  En,
  Ja,
  Ko,
  Es,
  Fr,
  De,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct Settings {
  enabled: bool,
  trigger_delay_ms: u64,
  ai_style: AiStyle,
  inference_mode: InferenceMode,
  output_language: OutputLanguage,
  request_on_boundary_only: bool,
  local_base_url: String,
  local_model_zh: String,
  local_model_en: String,
  local_max_tokens: u16,
  cloud_base_url: String,
  cloud_api_key: String,
  cloud_model: String,
  cloud_max_tokens: u16,
  hotkey_mode: HotkeyMode,
  custom_hotkey: String,
  autostart: bool,
  minimum_length: usize,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      enabled: true,
      trigger_delay_ms: 90,
      ai_style: AiStyle::Casual,
      inference_mode: InferenceMode::Cloud,
      output_language: OutputLanguage::Zh,
      request_on_boundary_only: false,
      local_base_url: DEFAULT_LOCAL_OLLAMA_URL.to_string(),
      local_model_zh: DEFAULT_LOCAL_MODEL_ZH.to_string(),
      local_model_en: DEFAULT_LOCAL_MODEL_EN.to_string(),
      local_max_tokens: DEFAULT_LOCAL_MAX_TOKENS,
      cloud_base_url: DEFAULT_CLOUD_CHAT_URL.to_string(),
      cloud_api_key: MVP_EMBEDDED_CLOUD_API_KEY.to_string(),
      cloud_model: DEFAULT_CLOUD_MODEL.to_string(),
      cloud_max_tokens: DEFAULT_CLOUD_MAX_TOKENS,
      hotkey_mode: HotkeyMode::Tab,
      custom_hotkey: "Ctrl+Shift+Space".to_string(),
      autostart: false,
      minimum_length: DEFAULT_MIN_TRIGGER_LEN,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageStats {
  day_key: String,
  used_today: u32,
  free_limit: u32,
}

impl Default for UsageStats {
  fn default() -> Self {
    Self {
      day_key: today_key(),
      used_today: 0,
      free_limit: FREE_DAILY_LIMIT,
    }
  }
}

impl UsageStats {
  fn refresh_day(&mut self) {
    let now = today_key();
    if self.day_key != now {
      self.day_key = now;
      self.used_today = 0;
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedState {
  settings: Settings,
  usage: UsageStats,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppSnapshot {
  settings: Settings,
  usage: UsageStats,
  suggestion: Option<String>,
  ghost_visible: bool,
  remaining_today: Option<u32>,
}

#[derive(Debug)]
struct RuntimeState {
  settings: Settings,
  usage: UsageStats,
  buffer: String,
  session_context: String,
  history: Vec<String>,
  prediction_seq: u64,
  last_input_was_boundary: bool,
  suggestion: Option<String>,
  ghost_visible: bool,
  pending_tab_accept: bool,
  pending_task: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl RuntimeState {
  fn snapshot(&self) -> AppSnapshot {
    AppSnapshot {
      settings: self.settings.clone(),
      usage: self.usage.clone(),
      suggestion: self.suggestion.clone(),
      ghost_visible: self.ghost_visible,
      remaining_today: None,
    }
  }
}

struct ManagedState {
  runtime: Arc<Mutex<RuntimeState>>,
  ai_client: Client,
}

#[derive(Debug, Clone)]
enum InputEvent {
  KeyPress {
    key: Key,
    text: Option<String>,
    ctrl: bool,
    alt: bool,
    shift: bool,
  },
  KeyRelease {
    key: Key,
  },
}

#[derive(Debug, Default)]
struct ModifierState {
  ctrl: bool,
  alt: bool,
  shift: bool,
}

#[derive(Debug, Clone, Copy)]
enum PredictionOutcome {
  NonEmpty,
  Empty,
  Error,
}

#[derive(Debug, Default)]
struct LatencyMetricsBucket {
  window_ms: VecDeque<u64>,
  total: u64,
  non_empty: u64,
  empty: u64,
  errors: u64,
}

#[derive(Debug)]
struct LatencyMetricsSummary {
  window_len: usize,
  total: u64,
  non_empty: u64,
  empty: u64,
  errors: u64,
  p50_ms: u64,
  p95_ms: u64,
  avg_ms: u64,
}

impl LatencyMetricsBucket {
  fn record(&mut self, latency_ms: u64, outcome: PredictionOutcome) -> Option<LatencyMetricsSummary> {
    self.total = self.total.saturating_add(1);
    match outcome {
      PredictionOutcome::NonEmpty => self.non_empty = self.non_empty.saturating_add(1),
      PredictionOutcome::Empty => self.empty = self.empty.saturating_add(1),
      PredictionOutcome::Error => self.errors = self.errors.saturating_add(1),
    }

    self.window_ms.push_back(latency_ms);
    while self.window_ms.len() > LATENCY_METRICS_WINDOW_SIZE {
      self.window_ms.pop_front();
    }

    if self.total % LATENCY_METRICS_LOG_EVERY != 0 || self.window_ms.is_empty() {
      return None;
    }

    let mut sorted = self.window_ms.iter().copied().collect::<Vec<_>>();
    sorted.sort_unstable();
    let sum: u128 = sorted.iter().map(|v| *v as u128).sum();
    let avg_ms = (sum / sorted.len() as u128) as u64;

    Some(LatencyMetricsSummary {
      window_len: sorted.len(),
      total: self.total,
      non_empty: self.non_empty,
      empty: self.empty,
      errors: self.errors,
      p50_ms: percentile_ms(&sorted, 50),
      p95_ms: percentile_ms(&sorted, 95),
      avg_ms,
    })
  }
}

#[derive(Debug, Default)]
struct LatencyMetricsStore {
  local: LatencyMetricsBucket,
  cloud: LatencyMetricsBucket,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GhostPayload {
  text: String,
  visible: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorPayload {
  message: String,
}

#[derive(Debug, Serialize)]
struct OllamaGenerateRequest {
  model: String,
  prompt: String,
  system: String,
  stream: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  keep_alive: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  raw: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  think: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
  temperature: f32,
  top_p: f32,
  num_predict: u16,
}

#[derive(Debug, Serialize)]
struct CloudChatRequest {
  model: String,
  messages: Vec<CloudChatMessage>,
  temperature: f32,
  top_p: f32,
  max_tokens: u16,
  stream: bool,
}

#[derive(Debug, Serialize)]
struct CloudChatMessage {
  role: String,
  content: String,
}

#[derive(Debug, Deserialize)]
struct CloudChatResponse {
  choices: Vec<CloudChatChoice>,
}

#[derive(Debug, Deserialize)]
struct CloudChatChoice {
  message: CloudChatMessageOut,
}

#[derive(Debug, Deserialize)]
struct CloudChatMessageOut {
  content: String,
}

#[tauri::command]
fn get_app_state(state: State<'_, Arc<ManagedState>>) -> AppSnapshot {
  let mut runtime = state.runtime.lock().expect("runtime poisoned");
  runtime.usage.refresh_day();
  runtime.snapshot()
}

#[tauri::command]
fn update_settings(
  app: AppHandle,
  state: State<'_, Arc<ManagedState>>,
  settings: Settings,
) -> Result<AppSnapshot, String> {
  let normalized = normalize_settings(settings);
  set_launch_on_startup(normalized.autostart)?;
  let mut should_hide = false;

  let snapshot = {
    let mut runtime = state.runtime.lock().map_err(|_| "runtime lock failed")?;
    runtime.usage.refresh_day();
    runtime.settings = normalized.clone();
    runtime.settings.autostart = is_launch_on_startup();

    if !runtime.settings.enabled {
      cancel_pending_locked(&mut runtime);
      runtime.buffer.clear();
      runtime.suggestion = None;
      runtime.ghost_visible = false;
      runtime.pending_tab_accept = false;
      should_hide = true;
    }

    persist_state(&app, &runtime.settings, &runtime.usage)?;
    runtime.snapshot()
  };

  if should_hide {
    hide_ghost_window(&app);
  }
  emit_state_event(&app, &state);
  Ok(snapshot)
}

#[tauri::command]
fn dismiss_ghost(app: AppHandle, state: State<'_, Arc<ManagedState>>) -> Result<(), String> {
  dismiss_suggestion(&app, &state);
  Ok(())
}

#[tauri::command]
fn accept_suggestion(app: AppHandle, state: State<'_, Arc<ManagedState>>) -> Result<bool, String> {
  let accepted = accept_current_suggestion(&app, &state, false)?;
  Ok(accepted)
}

#[tauri::command]
fn clear_usage(app: AppHandle, state: State<'_, Arc<ManagedState>>) -> Result<AppSnapshot, String> {
  let snapshot = {
    let mut runtime = state.runtime.lock().map_err(|_| "runtime lock failed")?;
    runtime.usage.day_key = today_key();
    runtime.usage.used_today = 0;
    persist_state(&app, &runtime.settings, &runtime.usage)?;
    runtime.snapshot()
  };

  emit_state_event(&app, &state);
  Ok(snapshot)
}

fn normalize_settings(mut settings: Settings) -> Settings {
  settings.trigger_delay_ms = if MVP_LOW_LATENCY_MODE {
    settings
      .trigger_delay_ms
      .clamp(MVP_TRIGGER_DELAY_MIN_MS, MVP_TRIGGER_DELAY_MAX_MS)
  } else {
    settings.trigger_delay_ms.clamp(200, 800)
  };
  let legacy_default_length = settings.minimum_length == LEGACY_DEFAULT_MIN_TRIGGER_LEN;
  if legacy_default_length {
    settings.minimum_length = DEFAULT_MIN_TRIGGER_LEN;
    if settings.output_language == OutputLanguage::Auto {
      settings.output_language = OutputLanguage::Zh;
    }
  }
  settings.minimum_length = if MVP_LOW_LATENCY_MODE {
    settings.minimum_length.clamp(DEFAULT_MIN_TRIGGER_LEN, 24)
  } else {
    settings.minimum_length.max(DEFAULT_MIN_TRIGGER_LEN)
  };
  settings.custom_hotkey = settings.custom_hotkey.trim().to_string();
  if RELAX_COMPLETION_GUARDS {
    settings.request_on_boundary_only = false;
  }
  settings.local_base_url = normalize_local_ollama_url(&settings.local_base_url);
  settings.local_model_zh = settings.local_model_zh.trim().to_string();
  settings.local_model_en = settings.local_model_en.trim().to_string();
  settings.local_max_tokens = if MVP_LOW_LATENCY_MODE {
    settings.local_max_tokens.clamp(8, MVP_MAX_OUTPUT_TOKENS)
  } else {
    settings.local_max_tokens.clamp(16, 256)
  };
  settings.cloud_base_url = normalize_cloud_chat_url(&settings.cloud_base_url);
  settings.cloud_api_key = settings.cloud_api_key.trim().to_string();
  settings.cloud_model = settings.cloud_model.trim().to_string();
  settings.cloud_max_tokens = if MVP_LOW_LATENCY_MODE {
    settings.cloud_max_tokens.clamp(8, MVP_MAX_OUTPUT_TOKENS)
  } else {
    settings.cloud_max_tokens.clamp(16, 256)
  };

  if settings.local_model_zh.is_empty() {
    settings.local_model_zh = DEFAULT_LOCAL_MODEL_ZH.to_string();
  }
  if settings.local_model_en.is_empty() {
    settings.local_model_en = DEFAULT_LOCAL_MODEL_EN.to_string();
  }
  if settings.cloud_model.is_empty() {
    settings.cloud_model = DEFAULT_CLOUD_MODEL.to_string();
  }
  if MVP_FORCE_CLOUD {
    settings.inference_mode = InferenceMode::Cloud;
  }
  if settings.inference_mode == InferenceMode::Local {
    let cloud_key = resolve_cloud_api_key(&settings.cloud_api_key);
    if !cloud_key.is_empty() {
      settings.inference_mode = InferenceMode::Cloud;
    }
  }

  settings
}

fn normalize_local_ollama_url(raw: &str) -> String {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return DEFAULT_LOCAL_OLLAMA_URL.to_string();
  }

  let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
    trimmed.to_string()
  } else {
    format!("http://{trimmed}")
  };
  let without_trailing_slash = with_scheme.trim_end_matches('/');
  let lowered = without_trailing_slash.to_ascii_lowercase();
  if lowered.ends_with("/api/generate") {
    without_trailing_slash.to_string()
  } else {
    format!("{without_trailing_slash}/api/generate")
  }
}

fn normalize_cloud_chat_url(raw: &str) -> String {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return DEFAULT_CLOUD_CHAT_URL.to_string();
  }

  let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
    trimmed.to_string()
  } else {
    format!("https://{trimmed}")
  };
  let without_trailing_slash = with_scheme.trim_end_matches('/');
  let lowered = without_trailing_slash.to_ascii_lowercase();
  if lowered.ends_with("/v1/chat/completions") {
    without_trailing_slash.to_string()
  } else {
    format!("{without_trailing_slash}/v1/chat/completions")
  }
}

fn state_file_path(app: &AppHandle) -> PathBuf {
  if let Ok(dir) = app.path().app_data_dir() {
    return dir.join(STATE_FILENAME);
  }
  PathBuf::from(STATE_FILENAME)
}

fn load_persisted_state(app: &AppHandle) -> PersistedState {
  let path = state_file_path(app);
  if !path.exists() {
    let mut default = PersistedState::default();
    default.settings.autostart = is_launch_on_startup();
    return default;
  }

  match fs::read_to_string(path) {
    Ok(data) => match serde_json::from_str::<PersistedState>(&data) {
      Ok(mut parsed) => {
        parsed.settings = normalize_settings(parsed.settings);
        parsed.settings.autostart = is_launch_on_startup();
        parsed.usage.refresh_day();
        parsed
      }
      Err(_) => {
        let mut default = PersistedState::default();
        default.settings.autostart = is_launch_on_startup();
        default
      }
    },
    Err(_) => {
      let mut default = PersistedState::default();
      default.settings.autostart = is_launch_on_startup();
      default
    }
  }
}

fn persist_state(app: &AppHandle, settings: &Settings, usage: &UsageStats) -> Result<(), String> {
  let path = state_file_path(app);
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|e| format!("failed to prepare config dir: {e}"))?;
  }

  let payload = PersistedState {
    settings: settings.clone(),
    usage: usage.clone(),
  };
  let data = serde_json::to_string_pretty(&payload).map_err(|e| format!("serialize failed: {e}"))?;
  fs::write(path, data).map_err(|e| format!("state save failed: {e}"))
}

fn emit_state_event(app: &AppHandle, managed: &Arc<ManagedState>) {
  let snapshot = {
    let mut runtime = match managed.runtime.lock() {
      Ok(value) => value,
      Err(_) => return,
    };
    runtime.usage.refresh_day();
    runtime.snapshot()
  };
  let _ = app.emit("typeace://state", snapshot);
}

fn emit_error_event(app: &AppHandle, message: impl Into<String>) {
  let _ = app.emit(
    "typeace://error",
    ErrorPayload {
      message: message.into(),
    },
  );
}

fn percentile_ms(sorted_ms: &[u64], percentile: u32) -> u64 {
  if sorted_ms.is_empty() {
    return 0;
  }
  let pct = percentile.clamp(1, 100) as usize;
  let len = sorted_ms.len();
  let rank = (len * pct).div_ceil(100);
  let idx = rank.saturating_sub(1).min(len - 1);
  sorted_ms[idx]
}

fn record_prediction_latency_metric(mode: InferenceMode, latency_ms: u64, outcome: PredictionOutcome) {
  let metrics = PREDICTION_LATENCY_METRICS.get_or_init(|| Mutex::new(LatencyMetricsStore::default()));
  let mut guard = match metrics.lock() {
    Ok(value) => value,
    Err(_) => return,
  };

  let bucket = match mode {
    InferenceMode::Local => &mut guard.local,
    InferenceMode::Cloud => &mut guard.cloud,
  };

  if let Some(summary) = bucket.record(latency_ms, outcome) {
    let non_empty_rate = if summary.total == 0 {
      0.0
    } else {
      (summary.non_empty as f64 * 100.0) / summary.total as f64
    };
    log::info!(
      "latency metrics mode={:?} window={} total={} non_empty={} empty={} error={} non_empty_rate={:.1}% p50={}ms p95={}ms avg={}ms",
      mode,
      summary.window_len,
      summary.total,
      summary.non_empty,
      summary.empty,
      summary.errors,
      non_empty_rate,
      summary.p50_ms,
      summary.p95_ms,
      summary.avg_ms
    );
  }
}

fn cancel_pending_locked(runtime: &mut RuntimeState) {
  if let Some(handle) = runtime.pending_task.take() {
    handle.abort();
  }
}

fn dismiss_suggestion(app: &AppHandle, managed: &Arc<ManagedState>) {
  let should_hide = {
    let mut runtime = match managed.runtime.lock() {
      Ok(value) => value,
      Err(_) => return,
    };
    let needed = runtime.suggestion.is_some() || runtime.ghost_visible;
    runtime.suggestion = None;
    runtime.ghost_visible = false;
    runtime.pending_tab_accept = false;
    needed
  };

  if should_hide {
    hide_ghost_window(app);
    emit_state_event(app, managed);
  }
}

fn accept_current_suggestion(
  app: &AppHandle,
  managed: &Arc<ManagedState>,
  remove_trigger_tab: bool,
) -> Result<bool, String> {
  let suggestion = {
    let mut runtime = managed.runtime.lock().map_err(|_| "runtime lock failed")?;
    runtime.usage.refresh_day();

    if !runtime.settings.enabled {
      return Ok(false);
    }

    match runtime.suggestion.clone() {
      Some(value) if !value.trim().is_empty() => value,
      _ => return Ok(false),
    }
  };

  log::info!("accept_current_suggestion triggered");
  hide_ghost_window(app);
  inject_text_via_paste(&suggestion, remove_trigger_tab)?;

  {
    let mut runtime = managed.runtime.lock().map_err(|_| "runtime lock failed")?;
    runtime.usage.refresh_day();
    runtime.usage.used_today = runtime.usage.used_today.saturating_add(1);
    runtime.buffer.push_str(&suggestion);
    runtime.buffer = trim_buffer_to_recent(&runtime.buffer, MAX_CONTEXT_CHARS);
    append_session_context(&mut runtime, &suggestion);
    runtime.suggestion = None;
    runtime.ghost_visible = false;
    runtime.pending_tab_accept = false;
    persist_state(app, &runtime.settings, &runtime.usage)?;
  }

  hide_ghost_window(app);
  emit_state_event(app, managed);
  Ok(true)
}

fn inject_text_via_paste(text: &str, remove_trigger_tab: bool) -> Result<(), String> {
  log::info!("inject_text_via_paste start, chars={}", text.chars().count());
  let inject_text = text.to_string();
  let worker = std::thread::spawn(move || -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard unavailable: {e}"))?;
    let previous = clipboard.get_text().ok();

    clipboard
      .set_text(inject_text)
      .map_err(|e| format!("failed writing clipboard: {e}"))?;
    std::thread::sleep(Duration::from_millis(80));

    if remove_trigger_tab {
      if let Err(err) = send_backspace() {
        log::warn!("send_backspace failed: {}", err);
      } else {
        std::thread::sleep(Duration::from_millis(20));
      }
    }

    send_paste_shortcut()?;

    std::thread::sleep(Duration::from_millis(180));
    if let Some(old) = previous {
      let _ = clipboard.set_text(old);
    }
    log::info!("inject_text_via_paste simulate done");
    Ok(())
  });

  worker
    .join()
    .map_err(|_| "paste worker panicked".to_string())?
}

fn simulate_ctrl_v_rdev() -> Result<(), String> {
  simulate(&EventType::KeyPress(Key::ControlLeft)).map_err(|e| format!("simulate key failed: {e:?}"))?;
  std::thread::sleep(Duration::from_millis(8));
  simulate(&EventType::KeyPress(Key::KeyV)).map_err(|e| format!("simulate key failed: {e:?}"))?;
  std::thread::sleep(Duration::from_millis(8));
  simulate(&EventType::KeyRelease(Key::KeyV)).map_err(|e| format!("simulate key failed: {e:?}"))?;
  std::thread::sleep(Duration::from_millis(8));
  simulate(&EventType::KeyRelease(Key::ControlLeft)).map_err(|e| format!("simulate key failed: {e:?}"))
}

fn simulate_backspace_rdev() -> Result<(), String> {
  simulate(&EventType::KeyPress(Key::Backspace)).map_err(|e| format!("simulate key failed: {e:?}"))?;
  std::thread::sleep(Duration::from_millis(8));
  simulate(&EventType::KeyRelease(Key::Backspace)).map_err(|e| format!("simulate key failed: {e:?}"))
}

#[cfg(target_os = "windows")]
fn send_paste_shortcut() -> Result<(), String> {
  match send_ctrl_v_windows() {
    Ok(()) => Ok(()),
    Err(err) => {
      log::warn!("SendInput Ctrl+V failed, fallback to rdev: {}", err);
      simulate_ctrl_v_rdev()
    }
  }
}

#[cfg(not(target_os = "windows"))]
fn send_paste_shortcut() -> Result<(), String> {
  simulate_ctrl_v_rdev()
}

#[cfg(target_os = "windows")]
fn send_backspace() -> Result<(), String> {
  match send_key_tap_windows(0x08) {
    Ok(()) => Ok(()),
    Err(err) => {
      log::warn!("SendInput Backspace failed, fallback to rdev: {}", err);
      simulate_backspace_rdev()
    }
  }
}

#[cfg(not(target_os = "windows"))]
fn send_backspace() -> Result<(), String> {
  simulate_backspace_rdev()
}

#[cfg(target_os = "windows")]
fn send_ctrl_v_windows() -> Result<(), String> {
  use windows::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, VK_CONTROL};

  send_key_chord_windows(VK_CONTROL.0, 0x56, KEYEVENTF_KEYUP)
}

#[cfg(target_os = "windows")]
fn send_key_tap_windows(vk: u16) -> Result<(), String> {
  use windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_KEYUP;
  send_input_windows(&[key_input(vk, 0), key_input(vk, KEYEVENTF_KEYUP.0)])
}

#[cfg(target_os = "windows")]
fn send_key_chord_windows(mod_vk: u16, key_vk: u16, keyup_flag: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS) -> Result<(), String> {
  send_input_windows(&[
    key_input(mod_vk, 0),
    key_input(key_vk, 0),
    key_input(key_vk, keyup_flag.0),
    key_input(mod_vk, keyup_flag.0),
  ])
}

#[cfg(target_os = "windows")]
fn key_input(vk: u16, flags: u32) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
  use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, VIRTUAL_KEY};

  INPUT {
    r#type: INPUT_KEYBOARD,
    Anonymous: INPUT_0 {
      ki: KEYBDINPUT {
        wVk: VIRTUAL_KEY(vk),
        wScan: 0,
        dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(flags),
        time: 0,
        dwExtraInfo: 0,
      },
    },
  }
}

#[cfg(target_os = "windows")]
fn send_input_windows(inputs: &[windows::Win32::UI::Input::KeyboardAndMouse::INPUT]) -> Result<(), String> {
  use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};

  let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
  if sent == 0 {
    return Err("SendInput returned 0".to_string());
  }

  if sent != inputs.len() as u32 {
    return Err(format!("SendInput partial send: {sent}/{}", inputs.len()));
  }
  Ok(())
}

fn trim_buffer_to_recent(buffer: &str, limit: usize) -> String {
  let chars: Vec<char> = buffer.chars().collect();
  if chars.len() <= limit {
    return buffer.to_string();
  }
  chars[chars.len() - limit..].iter().collect()
}

fn normalize_history_entry(value: &str) -> String {
  let collapsed = value
    .replace('\r', " ")
    .replace('\n', " ")
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");
  trim_buffer_to_recent(collapsed.trim(), MAX_CONTEXT_CHARS)
}

fn push_history_entry(runtime: &mut RuntimeState, value: &str) {
  let entry = normalize_history_entry(value);
  if entry.chars().count() < 4 {
    return;
  }
  if runtime.history.last().is_some_and(|last| last == &entry) {
    return;
  }

  runtime.history.push(entry);
  if runtime.history.len() > MAX_HISTORY_ITEMS {
    let remove_count = runtime.history.len() - MAX_HISTORY_ITEMS;
    runtime.history.drain(0..remove_count);
  }
}

fn build_recent_history(history: &[String]) -> String {
  if history.is_empty() {
    return String::new();
  }
  let merged = history.join("\n");
  trim_buffer_to_recent(&merged, MAX_HISTORY_CONTEXT_CHARS)
}

fn append_session_context(runtime: &mut RuntimeState, text: &str) {
  runtime.session_context.push_str(text);
  runtime.session_context = trim_buffer_to_recent(&runtime.session_context, MAX_SESSION_CONTEXT_CHARS);
}

fn extract_current_sentence_tail(context: &str) -> String {
  if context.is_empty() {
    return String::new();
  }

  let mut start = 0usize;
  for (idx, ch) in context.char_indices() {
    if matches!(
      ch,
      '\n' | '\r' | '.' | '!' | '?' | ',' | ';' | ':' | '。' | '！' | '？' | '，' | '；' | '：'
    ) {
      start = idx + ch.len_utf8();
    }
  }
  trim_buffer_to_recent(context[start..].trim(), MAX_SENTENCE_TAIL_CHARS)
}

fn is_commit_boundary_char(ch: char) -> bool {
  ch.is_whitespace()
    || matches!(
      ch,
      '.' | ',' | '!' | '?' | ';' | ':' | '，' | '。' | '！' | '？' | '；' | '：' | ')' | '）' | ']' | '】'
    )
}

fn is_commit_boundary_text(text: &str) -> bool {
  text.chars().any(is_commit_boundary_char)
}

fn looks_like_pinyin_tail(value: &str) -> bool {
  let trimmed = value.trim();
  if trimmed.chars().count() < 3 {
    return false;
  }
  if contains_cjk(trimmed) {
    return false;
  }
  let mut normalized = String::with_capacity(trimmed.len());
  for ch in trimmed.chars() {
    if ch.is_ascii_alphabetic() || ch.is_ascii_digit() || ch.is_ascii_whitespace() || ch == '\'' {
      normalized.push(ch);
    } else if matches!(ch, ',' | '.' | ';' | ':' | '!' | '?' | '-' | '_' | '/' | '\\') {
      normalized.push(' ');
    } else {
      return false;
    }
  }

  let tokens: Vec<String> = normalized
    .split(|ch: char| ch.is_ascii_whitespace() || ch == '\'')
    .map(|part| part.chars().filter(|ch| ch.is_ascii_alphabetic()).collect::<String>())
    .filter(|part| !part.is_empty())
    .collect();
  if tokens.is_empty() {
    return false;
  }
  if tokens.iter().any(|token| token.len() > 10) {
    return false;
  }

  let letters = normalized.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
  if letters < 3 {
    return false;
  }
  let vowels = normalized
    .chars()
    .filter(|ch| matches!(ch.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u'))
    .count();
  let vowel_ratio = vowels as f32 / letters as f32;
  vowel_ratio >= 0.18
}

fn is_likely_latin_output_for_zh(value: &str) -> bool {
  let trimmed = value.trim();
  if trimmed.is_empty() {
    return false;
  }
  if contains_cjk(trimmed) {
    return false;
  }

  let total = trimmed.chars().count();
  let ascii_letters = trimmed.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
  if ascii_letters < 3 {
    return false;
  }

  let latin_ratio = ascii_letters as f32 / total as f32;
  if latin_ratio < 0.45 {
    return false;
  }

  let tokens: Vec<&str> = trimmed
    .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '\'' || ch == '-'))
    .filter(|part| !part.is_empty())
    .collect();

  tokens.len() >= 2 || ascii_letters >= 8
}

fn contains_question_marker(value: &str) -> bool {
  value.contains('?') || value.contains('？')
}

fn should_track_input() -> bool {
  // Temporary behavior: disable focus-class safety filtering so IM/chat
  // clients (e.g. WeCom) are not blocked while testing completion flow.
  true
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
fn is_safe_text_focus_windows() -> bool {
  use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowLongPtrW, GetWindowThreadProcessId, GUITHREADINFO,
    ES_PASSWORD, GWL_STYLE,
  };

  unsafe {
    let foreground = GetForegroundWindow();
    if foreground.0.is_null() {
      return false;
    }

    let thread_id = GetWindowThreadProcessId(foreground, None);
    let mut gui = GUITHREADINFO::default();
    gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;

    if GetGUIThreadInfo(thread_id, &mut gui).is_err() {
      return false;
    }

    let focus = gui.hwndFocus;
    if focus.0.is_null() {
      return false;
    }

    let mut class_buf = [0u16; 256];
    let class_len = GetClassNameW(focus, &mut class_buf);
    if class_len <= 0 {
      return false;
    }

    let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]).to_ascii_lowercase();
    let style = GetWindowLongPtrW(focus, GWL_STYLE) as u32;
    if class_name == "edit" && (style & ES_PASSWORD as u32) != 0 {
      return false;
    }

    is_likely_text_class(&class_name)
  }
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
fn is_likely_text_class(class_name: &str) -> bool {
  matches!(
    class_name,
    "edit" | "richedit20w" | "richedit50w" | "scintilla" | "chrome_renderwidgethosthwnd"
  ) || class_name.contains("chrome")
    || class_name.contains("mozilla")
    || class_name.contains("webkit")
    || class_name.contains("notion")
    || class_name.contains("richedit")
    || class_name.contains("notepad")
}

fn ghost_anchor_position(allow_fallback: bool) -> Option<(i32, i32, &'static str)> {
  #[cfg(target_os = "windows")]
  {
    ghost_anchor_position_windows(allow_fallback)
  }

  #[cfg(not(target_os = "windows"))]
  {
    None
  }
}

#[cfg(target_os = "windows")]
fn ghost_anchor_position_windows(allow_fallback: bool) -> Option<(i32, i32, &'static str)> {
  if let Some((x, y)) = caret_position_windows() {
    return Some((x, y, "caret"));
  }

  if !allow_fallback {
    return None;
  }

  if let Some((x, y)) = mouse_position_windows() {
    return Some((x, y, "mouse"));
  }

  if let Some((x, y)) = read_last_ghost_anchor() {
    return Some((x, y, "last"));
  }

  None
}

#[cfg(target_os = "windows")]
fn caret_position_windows() -> Option<(i32, i32)> {
  use windows::Win32::{
    Foundation::POINT,
    Graphics::Gdi::ClientToScreen,
    UI::WindowsAndMessaging::{GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO},
  };

  unsafe {
    let foreground = GetForegroundWindow();
    if foreground.0.is_null() {
      return None;
    }

    let thread_id = GetWindowThreadProcessId(foreground, None);
    let mut gui = GUITHREADINFO::default();
    gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    if GetGUIThreadInfo(thread_id, &mut gui).is_err() {
      return None;
    }

    let target = if !gui.hwndCaret.0.is_null() {
      gui.hwndCaret
    } else {
      gui.hwndFocus
    };
    if target.0.is_null() {
      return None;
    }

    // Some apps report zeroed caret rects. That means unavailable.
    if gui.rcCaret.left == 0 && gui.rcCaret.top == 0 && gui.rcCaret.right == 0 && gui.rcCaret.bottom == 0 {
      return None;
    }

    let mut point = POINT {
      x: if gui.rcCaret.right > gui.rcCaret.left {
        gui.rcCaret.right
      } else {
        gui.rcCaret.left
      },
      y: gui.rcCaret.top,
    };

    if !ClientToScreen(target, &mut point).as_bool() {
      return None;
    }

    Some((point.x, point.y))
  }
}

#[cfg(target_os = "windows")]
fn mouse_position_windows() -> Option<(i32, i32)> {
  use windows::Win32::{
    Foundation::POINT,
    UI::WindowsAndMessaging::GetCursorPos,
  };

  unsafe {
    let mut point = POINT::default();
    if GetCursorPos(&mut point).is_err() {
      return None;
    }
    Some((point.x, point.y))
  }
}

fn remember_last_ghost_anchor(pos: (i32, i32)) {
  let cache = LAST_GHOST_ANCHOR.get_or_init(|| Mutex::new(None));
  if let Ok(mut guard) = cache.lock() {
    *guard = Some(pos);
  }
}

fn read_last_ghost_anchor() -> Option<(i32, i32)> {
  let cache = LAST_GHOST_ANCHOR.get_or_init(|| Mutex::new(None));
  let guard = cache.lock().ok()?;
  *guard
}

fn monitor_contains_point(monitor: &tauri::Monitor, x: i32, y: i32) -> bool {
  let pos = monitor.position();
  let size = monitor.size();
  x >= pos.x && x < pos.x + size.width as i32 && y >= pos.y && y < pos.y + size.height as i32
}

fn clamp_position_to_monitor(window: &tauri::WebviewWindow, x: i32, y: i32, width: i32, height: i32) -> (i32, i32) {
  let monitor = window
    .available_monitors()
    .ok()
    .and_then(|monitors| {
      monitors
        .into_iter()
        .find(|monitor| monitor_contains_point(monitor, x, y))
    })
    .or_else(|| window.current_monitor().ok().flatten());
  let Some(monitor) = monitor else {
    return (x, y);
  };

  let monitor_pos = monitor.position();
  let monitor_size = monitor.size();

  let min_x = monitor_pos.x;
  let min_y = monitor_pos.y;
  let max_x = monitor_pos.x + monitor_size.width as i32 - width;
  let max_y = monitor_pos.y + monitor_size.height as i32 - height;

  let clamped_x = x.clamp(min_x, max_x.max(min_x));
  let clamped_y = y.clamp(min_y, max_y.max(min_y));
  (clamped_x, clamped_y)
}

fn estimate_ghost_width(text: &str) -> i32 {
  let measured = text
    .chars()
    .take(120)
    .map(|ch| {
      if ch.is_whitespace() {
        5
      } else if is_cjk_char(ch) {
        14
      } else if ch.is_ascii() {
        8
      } else {
        10
      }
    })
    .sum::<i32>()
    + 20;
  measured.clamp(GHOST_MIN_WIDTH, GHOST_MAX_WIDTH)
}

fn is_cjk_char(ch: char) -> bool {
  ('\u{3400}'..='\u{4DBF}').contains(&ch)
    || ('\u{4E00}'..='\u{9FFF}').contains(&ch)
    || ('\u{F900}'..='\u{FAFF}').contains(&ch)
    || ('\u{3040}'..='\u{30FF}').contains(&ch)
    || ('\u{AC00}'..='\u{D7AF}').contains(&ch)
}

fn ensure_ghost_window(app: &AppHandle) -> Result<(), String> {
  if app.get_webview_window("ghost").is_some() {
    return Ok(());
  }

  let window = tauri::WebviewWindowBuilder::new(app, "ghost", tauri::WebviewUrl::App("ghost".into()))
    .title("TypeAce Ghost")
    .visible(false)
    .focusable(false)
    .focused(false)
    .decorations(false)
    .shadow(false)
    .always_on_top(true)
    .resizable(false)
    .transparent(true)
    .skip_taskbar(true)
    .inner_size(GHOST_MIN_WIDTH as f64, GHOST_HEIGHT as f64)
    .build()
    .map_err(|e| format!("failed creating ghost window: {e}"))?;
  window
    .set_ignore_cursor_events(true)
    .map_err(|e| format!("failed enabling ghost click-through: {e}"))?;

  Ok(())
}

fn position_ghost_window(
  window: &tauri::WebviewWindow,
  text: &str,
  allow_fallback: bool,
) -> Option<(i32, i32, &'static str)> {
  let Some((x, y, source)) = ghost_anchor_position(allow_fallback) else {
    return None;
  };

  if source != "last" {
    // Keep an un-offset anchor so fallback does not accumulate drift.
    remember_last_ghost_anchor((x, y));
  }

  let raw_x = x + GHOST_OFFSET_X;
  let raw_y = y + GHOST_OFFSET_Y;
  let ghost_width = estimate_ghost_width(text);
  let _ = window.set_size(Size::Physical(PhysicalSize::new(
    ghost_width as u32,
    GHOST_HEIGHT as u32,
  )));
  let (final_x, final_y) = clamp_position_to_monitor(window, raw_x, raw_y, ghost_width, GHOST_HEIGHT);
  let _ = window.set_position(Position::Physical(PhysicalPosition::new(
    final_x,
    final_y,
  )));
  Some((final_x, final_y, source))
}

fn refresh_ghost_window_position(app: &AppHandle, text: &str) -> Option<&'static str> {
  if let Some(window) = app.get_webview_window("ghost") {
    let _ = window.set_ignore_cursor_events(true);
    return position_ghost_window(&window, text, true).map(|(_, _, source)| source);
  }
  None
}

fn show_ghost_window(app: &AppHandle, text: &str) {
  if let Err(err) = ensure_ghost_window(app) {
    emit_error_event(app, err);
    return;
  }

  if let Some(window) = app.get_webview_window("ghost") {
    let _ = window.set_ignore_cursor_events(true);
    let Some((final_x, final_y, source)) = position_ghost_window(&window, text, true) else {
      log::info!("show_ghost_window skipped: position unavailable");
      let _ = window.hide();
      return;
    };
    let ghost_width = estimate_ghost_width(text);
    log::info!(
      "show_ghost_window position=({}, {}) source={} size=({}, {})",
      final_x,
      final_y,
      source,
      ghost_width,
      GHOST_HEIGHT
    );

    let _ = window.emit(
      "typeace://ghost",
      GhostPayload {
        text: text.to_string(),
        visible: true,
      },
    );
    let _ = window.show();
  }
}

fn hide_ghost_window(app: &AppHandle) {
  if let Some(window) = app.get_webview_window("ghost") {
    let _ = window.emit(
      "typeace://ghost",
      GhostPayload {
        text: String::new(),
        visible: false,
      },
    );
    let _ = window.hide();
  }
}

async fn run_ghost_follow_loop(app: AppHandle, managed: Arc<ManagedState>) {
  let mut ticker = tokio::time::interval(Duration::from_millis(24));
  loop {
    ticker.tick().await;
    let suggestion = {
      let runtime = match managed.runtime.lock() {
        Ok(value) => value,
        Err(_) => continue,
      };
      if runtime.ghost_visible {
        runtime.suggestion.clone()
      } else {
        None
      }
      };

    if let Some(text) = suggestion {
      let source = refresh_ghost_window_position(&app, &text);
      if source.is_none() {
        // Keep the last visible ghost position instead of aggressively hiding.
        // This avoids sudden disappearance in apps where caret API is unstable.
        continue;
      }
    }
  }
}

fn schedule_prediction(app: AppHandle, managed: Arc<ManagedState>, context: String) {
  let (delay_ms, settings_snapshot, history_snapshot, session_snapshot, boundary_snapshot, prediction_seq) = {
    let mut runtime = match managed.runtime.lock() {
      Ok(value) => value,
      Err(_) => return,
    };
    if MVP_LOW_LATENCY_MODE || !RELAX_COMPLETION_GUARDS {
      cancel_pending_locked(&mut runtime);
    }
    runtime.prediction_seq = runtime.prediction_seq.wrapping_add(1);
    let history = build_recent_history(&runtime.history);
    let session = runtime.session_context.clone();
    let effective_delay = if MVP_LOW_LATENCY_MODE {
      runtime
        .settings
        .trigger_delay_ms
        .clamp(MVP_TRIGGER_DELAY_MIN_MS, MVP_TRIGGER_DELAY_MAX_MS)
    } else {
      runtime.settings.trigger_delay_ms.clamp(200, 800)
    };
    let session_limit = if MVP_LOW_LATENCY_MODE {
      MVP_MAX_PROMPT_CHARS
    } else {
      MAX_SESSION_CONTEXT_CHARS
    };
    (
      effective_delay,
      runtime.settings.clone(),
      history,
      trim_buffer_to_recent(&session, session_limit),
      runtime.last_input_was_boundary,
      runtime.prediction_seq,
    )
  };

  let app_for_task = app.clone();
  let managed_for_task = managed.clone();
  let handle = tauri::async_runtime::spawn(async move {
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;

    if !should_track_input() {
      log::info!("schedule_prediction skipped: focus not safe text");
      return;
    }

    {
      let runtime = match managed_for_task.runtime.lock() {
        Ok(value) => value,
        Err(_) => return,
      };
      if runtime.prediction_seq != prediction_seq {
        return;
      }
      if !runtime.settings.enabled {
        log::info!("schedule_prediction skipped: disabled");
        return;
      }
      if runtime.buffer != context {
        // The buffer changed after this task was queued; let the latest task handle prediction.
        return;
      }
      if runtime.buffer.chars().count() < runtime.settings.minimum_length {
        log::info!("schedule_prediction skipped: below min length");
        return;
      }
    }

    if !RELAX_COMPLETION_GUARDS && settings_snapshot.request_on_boundary_only && !boundary_snapshot {
      log::info!("schedule_prediction skipped: waiting commit boundary");
      return;
    }

    log::info!(
      "request_prediction with {} chars (history={}, session={}, boundary={})",
      context.chars().count(),
      history_snapshot.chars().count(),
      session_snapshot.chars().count(),
      boundary_snapshot
    );
    let inference_mode = settings_snapshot.inference_mode;
    let request_started = Instant::now();
    let response = request_prediction(
      &managed_for_task.ai_client,
      &settings_snapshot,
      &context,
      &history_snapshot,
      &session_snapshot,
      boundary_snapshot,
    )
    .await;
    let latency_ms = request_started
      .elapsed()
      .as_millis()
      .min(u64::MAX as u128) as u64;
    let prediction = match response {
      Ok(value) => value,
      Err(err) => {
        record_prediction_latency_metric(inference_mode, latency_ms, PredictionOutcome::Error);
        log::error!("request_prediction failed: {}", err);
        emit_error_event(&app_for_task, err);
        return;
      }
    };

    if prediction.trim().is_empty() {
      record_prediction_latency_metric(inference_mode, latency_ms, PredictionOutcome::Empty);
      log::info!("request_prediction returned empty");
      return;
    }
    record_prediction_latency_metric(inference_mode, latency_ms, PredictionOutcome::NonEmpty);

    {
      let mut runtime = match managed_for_task.runtime.lock() {
        Ok(value) => value,
        Err(_) => return,
      };
      if runtime.prediction_seq != prediction_seq {
        return;
      }
      if !runtime.settings.enabled {
        log::info!("prediction dropped: disabled");
        return;
      }
      if !RELAX_COMPLETION_GUARDS && runtime.buffer != context {
        log::info!("prediction dropped: state changed");
        return;
      }

      runtime.usage.refresh_day();

      runtime.suggestion = Some(prediction.clone());
      runtime.ghost_visible = true;
      runtime.pending_tab_accept = false;
      runtime.pending_task = None;
    }

    log::info!("prediction ready: {}", prediction);
    show_ghost_window(&app_for_task, &prediction);
    emit_state_event(&app_for_task, &managed_for_task);
  });

  if let Ok(mut runtime) = managed.runtime.lock() {
    runtime.pending_task = Some(handle);
  }
}

fn normalize_input_text(raw: Option<String>) -> Option<String> {
  let value = raw?;
  if value.is_empty() {
    return None;
  }

  let cleaned = value.replace('\r', "");
  if cleaned
    .chars()
    .all(|ch| !ch.is_control() || ch == '\n' || ch == '\t')
  {
    Some(cleaned)
  } else {
    None
  }
}

fn update_context_with_text(app: &AppHandle, managed: &Arc<ManagedState>, text: &str) {
  if !should_track_input() {
    log::info!("update_context_with_text skipped: focus not safe text");
    let mut runtime = match managed.runtime.lock() {
      Ok(value) => value,
      Err(_) => return,
    };
    runtime.buffer.clear();
    runtime.suggestion = None;
    runtime.ghost_visible = false;
    runtime.pending_tab_accept = false;
    runtime.last_input_was_boundary = false;
    cancel_pending_locked(&mut runtime);
    hide_ghost_window(app);
    return;
  }

  let context = {
    let mut runtime = match managed.runtime.lock() {
      Ok(value) => value,
      Err(_) => return,
    };

    if !runtime.settings.enabled {
      return;
    }

    runtime.buffer.push_str(text);
    append_session_context(&mut runtime, text);
    runtime.buffer = trim_buffer_to_recent(&runtime.buffer, MAX_CONTEXT_CHARS);
    runtime.last_input_was_boundary = is_commit_boundary_text(text);
    log::info!("buffer updated, len={}", runtime.buffer.chars().count());
    runtime.suggestion = None;
    runtime.ghost_visible = false;
    runtime.pending_tab_accept = false;

    if runtime.buffer.chars().count() < runtime.settings.minimum_length {
      cancel_pending_locked(&mut runtime);
      return;
    }

    runtime.buffer.clone()
  };

  hide_ghost_window(app);
  schedule_prediction(app.clone(), managed.clone(), context);
}

fn update_context_backspace(app: &AppHandle, managed: &Arc<ManagedState>) {
  let context = {
    let mut runtime = match managed.runtime.lock() {
      Ok(value) => value,
      Err(_) => return,
    };

    if !runtime.settings.enabled {
      return;
    }

    runtime.buffer.pop();
    runtime.session_context.pop();
    runtime.suggestion = None;
    runtime.ghost_visible = false;
    runtime.pending_tab_accept = false;
    runtime.last_input_was_boundary = false;

    if runtime.buffer.chars().count() < runtime.settings.minimum_length {
      cancel_pending_locked(&mut runtime);
      return;
    }

    runtime.buffer.clone()
  };

  hide_ghost_window(app);
  schedule_prediction(app.clone(), managed.clone(), context);
}

fn is_modifier_key(key: Key) -> bool {
  matches!(
    key,
    Key::ControlLeft
      | Key::ControlRight
      | Key::ShiftLeft
      | Key::ShiftRight
      | Key::Alt
      | Key::AltGr
      | Key::MetaLeft
      | Key::MetaRight
  )
}

fn should_accept_hotkey(
  runtime: &RuntimeState,
  key: Key,
  text: &Option<String>,
  ctrl: bool,
  alt: bool,
  shift: bool,
) -> bool {
  if runtime.suggestion.is_none() || !runtime.settings.enabled {
    return false;
  }

  match runtime.settings.hotkey_mode {
    HotkeyMode::Tab => key == Key::Tab,
    HotkeyMode::CtrlSpace => key == Key::Space && ctrl,
    HotkeyMode::Custom => custom_hotkey_matches(&runtime.settings.custom_hotkey, key, text, ctrl, alt, shift),
  }
}

fn custom_hotkey_matches(
  pattern: &str,
  key: Key,
  text: &Option<String>,
  ctrl: bool,
  alt: bool,
  shift: bool,
) -> bool {
  let lowered: Vec<String> = pattern
    .split('+')
    .map(|part| part.trim().to_ascii_lowercase())
    .filter(|part| !part.is_empty())
    .collect();

  if lowered.is_empty() {
    return false;
  }

  let expects_ctrl = lowered.iter().any(|p| p == "ctrl" || p == "control");
  let expects_alt = lowered.iter().any(|p| p == "alt");
  let expects_shift = lowered.iter().any(|p| p == "shift");
  let primary = lowered
    .iter()
    .find(|p| *p != "ctrl" && *p != "control" && *p != "alt" && *p != "shift")
    .cloned()
    .unwrap_or_else(|| "space".to_string());

  if ctrl != expects_ctrl || alt != expects_alt || shift != expects_shift {
    return false;
  }

  match primary.as_str() {
    "tab" => key == Key::Tab,
    "space" => key == Key::Space,
    "enter" | "return" => key == Key::Return,
    value if value.len() == 1 => {
      let ch = value.chars().next();
      text
        .as_ref()
        .and_then(|raw| raw.chars().next())
        .zip(ch)
        .map(|(actual, expected)| actual.eq_ignore_ascii_case(&expected))
        .unwrap_or(false)
    }
    _ => false,
  }
}

fn start_global_listener(sender: mpsc::UnboundedSender<InputEvent>) {
  std::thread::spawn(move || {
    let modifier_state = Arc::new(Mutex::new(ModifierState::default()));
    let modifier_ref = modifier_state.clone();

    let callback = move |event: Event| match event.event_type {
      EventType::KeyPress(key) => {
        let (ctrl, alt, shift) = {
          let mut modifier = match modifier_ref.lock() {
            Ok(value) => value,
            Err(_) => return,
          };
          match key {
            Key::ControlLeft | Key::ControlRight => modifier.ctrl = true,
            Key::Alt | Key::AltGr => modifier.alt = true,
            Key::ShiftLeft | Key::ShiftRight => modifier.shift = true,
            _ => {}
          }
          (modifier.ctrl, modifier.alt, modifier.shift)
        };

        let _ = sender.send(InputEvent::KeyPress {
          key,
          text: event.name.clone(),
          ctrl,
          alt,
          shift,
        });
      }
      EventType::KeyRelease(key) => {
        {
          let mut modifier = match modifier_ref.lock() {
            Ok(value) => value,
            Err(_) => return,
          };
          match key {
            Key::ControlLeft | Key::ControlRight => modifier.ctrl = false,
            Key::Alt | Key::AltGr => modifier.alt = false,
            Key::ShiftLeft | Key::ShiftRight => modifier.shift = false,
            _ => {}
          }
        }

        let _ = sender.send(InputEvent::KeyRelease { key });
      }
      _ => {}
    };

    if let Err(err) = listen(callback) {
      eprintln!("TypeAce global listener failed: {err:?}");
    }
  });
}

async fn process_input_events(
  app: AppHandle,
  managed: Arc<ManagedState>,
  mut receiver: mpsc::UnboundedReceiver<InputEvent>,
) {
  while let Some(event) = receiver.recv().await {
    match event {
      InputEvent::KeyPress {
        key,
        text,
        ctrl,
        alt,
        shift,
      } => {
        if key == Key::Escape {
          log::info!("global key: Escape");
          dismiss_suggestion(&app, &managed);
          continue;
        }

        let should_accept = {
          let runtime = match managed.runtime.lock() {
            Ok(value) => value,
            Err(_) => continue,
          };
          should_accept_hotkey(&runtime, key, &text, ctrl, alt, shift)
        };

        if should_accept {
          let is_tab_key = key == Key::Tab;
          if is_tab_key {
            if let Ok(mut runtime) = managed.runtime.lock() {
              runtime.pending_tab_accept = true;
            }
            log::info!("global hotkey matched on Tab, waiting key release");
          } else {
            log::info!("global hotkey matched, accepting suggestion");
            if let Err(err) = accept_current_suggestion(&app, &managed, false) {
              emit_error_event(&app, err);
            }
          }
          continue;
        }

        if key == Key::Backspace {
          log::info!("global key: Backspace");
          update_context_backspace(&app, &managed);
          continue;
        }

        if key == Key::Return {
          log::info!("global key: Return");
          let mut runtime = match managed.runtime.lock() {
            Ok(value) => value,
            Err(_) => continue,
          };
          if !runtime.buffer.trim().is_empty() {
            let snapshot = runtime.buffer.clone();
            push_history_entry(&mut runtime, &snapshot);
          }
          append_session_context(&mut runtime, "\n");
          runtime.buffer.clear();
          runtime.last_input_was_boundary = true;
          cancel_pending_locked(&mut runtime);
          continue;
        }

        if key == Key::Tab {
          log::info!("global key: Tab");
          let mut runtime = match managed.runtime.lock() {
            Ok(value) => value,
            Err(_) => continue,
          };
          append_session_context(&mut runtime, "\t");
          runtime.buffer.clear();
          runtime.last_input_was_boundary = true;
          cancel_pending_locked(&mut runtime);
          continue;
        }

        if is_modifier_key(key) {
          continue;
        }

        if let Some(value) = normalize_input_text(text) {
          log::info!("global text input: {}", value);
          update_context_with_text(&app, &managed, &value);
        }
      }
      InputEvent::KeyRelease { key } => {
        if key == Key::Escape {
          dismiss_suggestion(&app, &managed);
          continue;
        }

        if key == Key::Tab {
          let should_accept = {
            let mut runtime = match managed.runtime.lock() {
              Ok(value) => value,
              Err(_) => continue,
            };
            let pending = runtime.pending_tab_accept;
            runtime.pending_tab_accept = false;
            pending
          };

          if should_accept {
            log::info!("tab released, accepting suggestion");
            if let Err(err) = accept_current_suggestion(&app, &managed, true) {
              emit_error_event(&app, err);
            }
          }
        }
      }
    }
  }
}

async fn request_prediction(
  client: &Client,
  settings: &Settings,
  context: &str,
  recent_history: &str,
  full_context: &str,
  last_input_was_boundary: bool,
) -> Result<String, String> {
  let mock_enabled = std::env::var("TYPEACE_MOCK_COMPLETION")
    .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
    .unwrap_or(false);
  log::info!("request_prediction mock_enabled={}", mock_enabled);
  if mock_enabled {
    return Ok("work together on this project".to_string());
  }

  let sentence_tail = extract_current_sentence_tail(context);
  let pinyin_mode = looks_like_pinyin_tail(&sentence_tail);
  if !RELAX_COMPLETION_GUARDS
    && matches!(settings.output_language, OutputLanguage::Zh | OutputLanguage::Auto)
    && pinyin_mode
    && !last_input_was_boundary
  {
    log::info!("request_prediction skipped: pinyin composition pending commit boundary");
    return Ok(String::new());
  }

  let prompt_context_limit = if MVP_LOW_LATENCY_MODE {
    MVP_MAX_PROMPT_CHARS
  } else {
    MAX_SESSION_CONTEXT_CHARS
  };
  let history_context_limit = if MVP_LOW_LATENCY_MODE {
    MVP_MAX_HISTORY_CHARS
  } else {
    MAX_HISTORY_CONTEXT_CHARS
  };
  let full_context_hint = trim_buffer_to_recent(full_context, prompt_context_limit);
  let full_context_hint = if full_context_hint.trim().is_empty() {
    context
  } else {
    full_context_hint.as_str()
  };
  let prefer_zh_output = match settings.output_language {
    OutputLanguage::Zh => true,
    OutputLanguage::Auto => contains_cjk(context) || contains_cjk(full_context_hint) || pinyin_mode,
    _ => false,
  };
  let history_short = trim_buffer_to_recent(recent_history, history_context_limit);
  let history_hint = if history_short.trim().is_empty() {
    if prefer_zh_output { "无" } else { "(none)" }
  } else {
    history_short.as_str()
  };
  let sentence_tail_hint = if sentence_tail.is_empty() {
    context
  } else {
    sentence_tail.as_str()
  };
  let boundary_hint = if last_input_was_boundary {
    if prefer_zh_output { "是" } else { "yes" }
  } else if prefer_zh_output {
    "否"
  } else {
    "no"
  };
  let ime_hint = if pinyin_mode {
    "当前可能是拼音输入阶段，请基于上下文推断用户要输入的中文，并给出自然续写。"
  } else {
    ""
  };
  let (system_prompt, user_prompt) = if MVP_LOW_LATENCY_MODE {
    if prefer_zh_output {
      (
        "你是输入补全引擎。只输出可直接插入的一段中文续写。禁止解释，禁止拼音。",
        format!(
          "上文：{full_context_hint}\n\
最近：{history_hint}\n\
当前：{context}\n\
句尾：{sentence_tail_hint}\n\
{ime_hint}\n\
只输出续写："
        ),
      )
    } else {
      (
        "You are a typing completion engine. Return continuation only.",
        format!(
          "Context: {full_context_hint}\n\
Recent: {history_hint}\n\
Current: {context}\n\
Tail: {sentence_tail_hint}\n\
Continuation only:"
        ),
      )
    }
  } else {
    let style_hint = if prefer_zh_output {
      match settings.ai_style {
        AiStyle::Casual => "语气自然、口语化。",
        AiStyle::Professional => "语气简洁、专业。",
        AiStyle::Creative => "语气有创意但可读性强。",
      }
    } else {
      match settings.ai_style {
        AiStyle::Casual => "Continue naturally in a casual tone.",
        AiStyle::Professional => "Continue in a concise professional tone.",
        AiStyle::Creative => "Continue with an imaginative but readable tone.",
      }
    };
    let language_hint = if prefer_zh_output {
      "简体中文"
    } else {
      output_language_hint(settings.output_language)
    };
    if prefer_zh_output {
      (
        "你是本地输入补全引擎。只返回可直接插入到光标后的正文片段。禁止输出规则说明、提示词字段名、占位符或模板句。不要解释，不要复述，不要提问。保持与上文语义一致，优先给最稳妥的一种续写。",
        format!(
          "请基于上下文继续输入。\n\
只输出可直接插入到光标后的中文正文，不要解释、不要标签。\n\
风格：{style_hint}\n\
目标语言：{language_hint}\n\
是否在空格/标点后触发：{boundary_hint}\n\
输入法提示：{ime_hint}\n\
最近上下文：{history_hint}\n\
上文全文：{full_context_hint}\n\
当前输入：{context}\n\
当前句尾：{sentence_tail_hint}\n\
续写："
        ),
      )
    } else {
      (
        "You are a local typing completion engine. Return insertion-ready continuation only. No explanations, no labels, no template text.",
        format!(
          "Continue the user's text using context.\n\
Return continuation text only (no explanation, no labels).\n\
Style: {style_hint}\n\
Target language: {language_hint}\n\
Triggered on boundary: {boundary_hint}\n\
IME hint: {ime_hint}\n\
Recent context: {history_hint}\n\
Full context: {full_context_hint}\n\
Current input: {context}\n\
Current sentence tail: {sentence_tail_hint}\n\
Continuation:"
        ),
      )
    }
  };
  let cloud_api_key = resolve_cloud_api_key(&settings.cloud_api_key);
  let inference_mode = if MVP_FORCE_CLOUD {
    InferenceMode::Cloud
  } else if MVP_LOW_LATENCY_MODE && !cloud_api_key.is_empty() {
    InferenceMode::Cloud
  } else {
    settings.inference_mode
  };
  if inference_mode != settings.inference_mode {
    log::info!(
      "request_prediction override mode {:?} -> {:?} (force_cloud={} cloud_key_detected={})",
      settings.inference_mode,
      inference_mode,
      MVP_FORCE_CLOUD,
      !cloud_api_key.is_empty()
    );
  }
  let (endpoint, mut model) = match inference_mode {
    InferenceMode::Local => (
      normalize_local_ollama_url(&settings.local_base_url),
      select_local_model(settings, context, full_context),
    ),
    InferenceMode::Cloud => {
      let cloud_model = settings.cloud_model.trim();
      (
        normalize_cloud_chat_url(&settings.cloud_base_url),
        if cloud_model.is_empty() {
          DEFAULT_CLOUD_MODEL.to_string()
        } else {
          cloud_model.to_string()
        },
      )
    }
  };
  if pinyin_mode && matches!(settings.output_language, OutputLanguage::Auto) && inference_mode == InferenceMode::Local {
    let zh_model = settings.local_model_zh.trim();
    model = if zh_model.is_empty() {
      DEFAULT_LOCAL_MODEL_ZH.to_string()
    } else {
      zh_model.to_string()
    };
  }
  let disable_thinking = should_disable_thinking(&model);
  let base_temperature = completion_temperature(&settings.ai_style);
  let temperature = if MVP_LOW_LATENCY_MODE {
    if prefer_zh_output || prefer_model_is_zh(&model) {
      0.12
    } else {
      0.16
    }
  } else {
    base_temperature
  };
  let top_p = if MVP_LOW_LATENCY_MODE { 0.8 } else { 0.9 };
  let num_predict = if inference_mode == InferenceMode::Cloud {
    if MVP_LOW_LATENCY_MODE {
      settings.cloud_max_tokens.clamp(8, MVP_MAX_OUTPUT_TOKENS)
    } else {
      settings.cloud_max_tokens.clamp(16, 256)
    }
  } else if MVP_LOW_LATENCY_MODE {
    settings.local_max_tokens.clamp(8, MVP_MAX_OUTPUT_TOKENS)
  } else {
    settings.local_max_tokens.clamp(16, 256)
  };
  log::info!(
    "request_prediction mode={:?} endpoint={} model={} temperature={:.2} think_disabled={} mvp_fast={}",
    inference_mode,
    endpoint,
    model,
    temperature,
    disable_thinking,
    MVP_LOW_LATENCY_MODE
  );

  let raw = match inference_mode {
    InferenceMode::Local => {
      let payload = OllamaGenerateRequest {
        model: model.clone(),
        prompt: user_prompt.clone(),
        system: system_prompt.to_string(),
        stream: false,
        keep_alive: Some(MVP_KEEP_ALIVE.to_string()),
        raw: None,
        think: if disable_thinking {
          Some(false)
        } else {
          None
        },
        options: Some(OllamaOptions {
          temperature,
          top_p,
          num_predict,
        }),
      };
      let data = call_local_model_generate(client, &endpoint, &payload).await?;
      data
        .get("response")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
    }
    InferenceMode::Cloud => {
      let payload = CloudChatRequest {
        model: model.clone(),
        messages: vec![
          CloudChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
          },
          CloudChatMessage {
            role: "user".to_string(),
            content: user_prompt.clone(),
          },
        ],
        temperature,
        top_p,
        max_tokens: num_predict,
        stream: false,
      };
      call_cloud_model_chat(client, &endpoint, &cloud_api_key, &payload).await?
    }
  };
  if raw.is_empty() {
    log::info!("model returned empty response (mode={:?})", inference_mode);
    return Ok(String::new());
  }
  log::info!("model raw response chars={} (mode={:?})", raw.chars().count(), inference_mode);

  let candidates = vec![raw.clone()];
  let selected = select_best_candidate(
    context,
    recent_history,
    full_context,
    settings.output_language,
    &candidates,
  );
  let selected_requires_zh_retry = prefer_zh_output
    && !selected.trim().is_empty()
    && is_likely_latin_output_for_zh(&selected);
  if selected_requires_zh_retry {
    log::info!(
      "prediction requires zh retry: likely latin/pinyin output in zh mode, preview={}",
      preview_text_for_log(&selected, 80)
    );
  }
  if !selected.trim().is_empty() && !selected_requires_zh_retry {
    return Ok(selected);
  }

  let should_retry_compact = if RELAX_COMPLETION_GUARDS {
    raw.chars().count() <= 12 || selected_requires_zh_retry
  } else {
    is_prompt_echo_completion(&raw) || raw.chars().count() <= 12 || selected_requires_zh_retry
  };
  if should_retry_compact {
    log::info!("request_prediction retry: compact prompt fallback");
    let compact_system = if prefer_zh_output {
      "你是中文输入补全器。只输出可直接插入的简体中文续写正文。禁止拼音、禁止英文、禁止解释。"
    } else {
      "You are a typing completion engine. Output continuation text only."
    };
    let compact_prompt = if prefer_zh_output {
      format!(
        "请基于下面内容继续写自然的一小段。\n\
要求：必须包含汉字；不能输出拼音字母；不能输出解释。\n\
若上文是拼音输入，请直接给出对应中文续写。\n\
文本：{full_context_hint}\n\
当前输入：{context}\n\
续写："
      )
    } else {
      format!(
        "Continue the text naturally. Output continuation only.\n\
Text: {full_context_hint}\n\
Current input: {context}\n\
Continuation:"
      )
    };

    let retry_raw = match inference_mode {
      InferenceMode::Local => {
        let compact_payload = OllamaGenerateRequest {
          model: model.clone(),
          prompt: compact_prompt.clone(),
          system: compact_system.to_string(),
          stream: false,
          keep_alive: Some(MVP_KEEP_ALIVE.to_string()),
          raw: None,
          think: if disable_thinking {
            Some(false)
          } else {
            None
          },
          options: Some(OllamaOptions {
            temperature,
            top_p: if MVP_LOW_LATENCY_MODE { 0.78 } else { 0.85 },
            num_predict: if MVP_LOW_LATENCY_MODE {
              settings.local_max_tokens.clamp(8, MVP_MAX_OUTPUT_TOKENS)
            } else {
              settings.local_max_tokens.clamp(24, 128)
            },
          }),
        };
        let retry_data = call_local_model_generate(client, &endpoint, &compact_payload).await?;
        retry_data
          .get("response")
          .and_then(|value| value.as_str())
          .unwrap_or("")
          .trim()
          .to_string()
      }
      InferenceMode::Cloud => {
        let cloud_payload = CloudChatRequest {
          model: model.clone(),
          messages: vec![
            CloudChatMessage {
              role: "system".to_string(),
              content: compact_system.to_string(),
            },
            CloudChatMessage {
              role: "user".to_string(),
              content: compact_prompt.clone(),
            },
          ],
          temperature,
          top_p: if MVP_LOW_LATENCY_MODE { 0.78 } else { 0.85 },
          max_tokens: if MVP_LOW_LATENCY_MODE {
            settings.cloud_max_tokens.clamp(8, MVP_MAX_OUTPUT_TOKENS)
          } else {
            settings.cloud_max_tokens.clamp(24, 128)
          },
          stream: false,
        };
        call_cloud_model_chat(client, &endpoint, &cloud_api_key, &cloud_payload).await?
      }
    };

    if retry_raw.is_empty() {
      log::info!("compact retry returned empty (mode={:?})", inference_mode);
    } else {
      log::info!("compact retry raw response chars={}", retry_raw.chars().count());
      let retry_candidates = vec![retry_raw.clone()];
      let retry_selected = select_best_candidate(
        context,
        recent_history,
        full_context,
        settings.output_language,
        &retry_candidates,
      );
      let retry_requires_zh_retry = prefer_zh_output
        && !retry_selected.trim().is_empty()
        && is_likely_latin_output_for_zh(&retry_selected);
      if retry_requires_zh_retry {
        log::info!(
          "compact retry dropped: likely latin/pinyin output in zh mode, preview={}",
          preview_text_for_log(&retry_selected, 80)
        );
      }
      if !retry_selected.trim().is_empty() && !retry_requires_zh_retry {
        return Ok(retry_selected);
      }
    }
  }

  if pinyin_mode {
    let fallback = sanitize_completion(context, &candidates[0]);
    if contains_cjk(&fallback) {
      let fallback = trim_buffer_to_recent(fallback.trim(), 48);
      if !fallback.is_empty() {
        log::info!("prediction fallback used for pinyin mode");
        return Ok(fallback);
      }
    }
  }
  if selected_requires_zh_retry {
    return Ok(String::new());
  }
  Ok(selected)
}

async fn call_local_model_generate(
  client: &Client,
  endpoint: &str,
  payload: &OllamaGenerateRequest,
) -> Result<serde_json::Value, String> {
  let timeout_secs = if MVP_LOW_LATENCY_MODE {
    MVP_REQUEST_TIMEOUT_SECS
  } else {
    20
  };
  let response = client
    .post(endpoint)
    .json(payload)
    .timeout(Duration::from_secs(timeout_secs))
    .send()
    .await
    .map_err(|e| {
      format!(
        "local model request failed: {e}. 请确认 Ollama 已启动，且地址 `{}` 可访问。",
        endpoint
      )
    })?;

  if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_else(|_| String::new());
    return Err(format!(
      "local model rejected request ({status}): {body}. 请检查模型名是否已 `ollama pull`。"
    ));
  }

  let data = response
    .json::<serde_json::Value>()
    .await
    .map_err(|e| format!("invalid local model response: {e}"))?;

  if let Some(message) = data.get("error").and_then(|value| value.as_str()) {
    return Err(format!("local model returned error: {message}"));
  }

  Ok(data)
}

async fn call_cloud_model_chat(
  client: &Client,
  endpoint: &str,
  api_key: &str,
  payload: &CloudChatRequest,
) -> Result<String, String> {
  let trimmed_key = api_key.trim();
  if trimmed_key.is_empty() {
    return Err("Cloud mode requires API key. 请在设置中填写云端 API Key。".to_string());
  }

  let timeout_secs = if MVP_LOW_LATENCY_MODE {
    MVP_CLOUD_TIMEOUT_SECS
  } else {
    15
  };
  let response = client
    .post(endpoint)
    .bearer_auth(trimmed_key)
    .json(payload)
    .timeout(Duration::from_secs(timeout_secs))
    .send()
    .await
    .map_err(|e| format!("cloud request failed: {e}"))?;

  if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_else(|_| String::new());
    return Err(format!("cloud request rejected ({status}): {body}"));
  }

  let text = response.text().await.map_err(|e| format!("cloud body read failed: {e}"))?;
  let parsed: CloudChatResponse =
    serde_json::from_str(&text).map_err(|e| format!("cloud response parse failed: {e}; body={text}"))?;

  let value = parsed
    .choices
    .first()
    .map(|choice| choice.message.content.trim().to_string())
    .unwrap_or_default();
  Ok(value)
}

fn resolve_cloud_api_key(settings_key: &str) -> String {
  let key = settings_key.trim();
  if !key.is_empty() {
    return key.to_string();
  }
  if let Ok(value) = std::env::var("TYPEACE_CLOUD_API_KEY") {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
      return trimmed.to_string();
    }
  }
  if let Ok(value) = std::env::var("DEEPSEEK_API_KEY") {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
      return trimmed.to_string();
    }
  }
  if !MVP_EMBEDDED_CLOUD_API_KEY.trim().is_empty() {
    return MVP_EMBEDDED_CLOUD_API_KEY.trim().to_string();
  }
  String::new()
}

fn select_local_model(settings: &Settings, context: &str, full_context: &str) -> String {
  let is_cjk_context = contains_cjk(context) || contains_cjk(full_context);
  let preferred = match settings.output_language {
    OutputLanguage::Zh | OutputLanguage::Ja | OutputLanguage::Ko => settings.local_model_zh.trim(),
    OutputLanguage::En | OutputLanguage::Es | OutputLanguage::Fr | OutputLanguage::De => {
      settings.local_model_en.trim()
    }
    OutputLanguage::Auto => {
      if is_cjk_context {
        settings.local_model_zh.trim()
      } else {
        settings.local_model_en.trim()
      }
    }
  };

  if preferred.is_empty() {
    if is_cjk_context {
      DEFAULT_LOCAL_MODEL_ZH.to_string()
    } else {
      DEFAULT_LOCAL_MODEL_EN.to_string()
    }
  } else {
    preferred.to_string()
  }
}

fn completion_temperature(ai_style: &AiStyle) -> f32 {
  if matches!(ai_style, AiStyle::Creative) {
    0.35
  } else {
    0.2
  }
}

fn should_disable_thinking(model: &str) -> bool {
  let model_lc = model.to_ascii_lowercase();
  model_lc.contains("qwen")
}

fn prefer_model_is_zh(model: &str) -> bool {
  let model_lc = model.to_ascii_lowercase();
  model_lc.contains("qwen") || model_lc.contains("yi") || model_lc.contains("glm")
}

fn sanitize_completion(context: &str, raw: &str) -> String {
  let mut value = strip_reasoning_artifacts(raw);
  value = value.trim().replace('\r', " ").replace('\n', " ");
  if value.starts_with(context) {
    value = value[context.len()..].trim_start().to_string();
  }
  for prefix in [
    "Continuation:",
    "continuation:",
    "Output:",
    "output:",
    "Answer:",
    "answer:",
  ] {
    if value.starts_with(prefix) {
      value = value[prefix.len()..].trim_start().to_string();
    }
  }
  value = value.trim_matches('"').to_string();
  if !RELAX_COMPLETION_GUARDS && is_prompt_echo_completion(&value) {
    log::info!(
      "completion dropped: prompt echo/template text, preview={}",
      preview_text_for_log(&value, 80)
    );
    return String::new();
  }
  trim_buffer_to_recent(&value, 96)
}

fn preview_text_for_log(value: &str, limit: usize) -> String {
  let mut preview = value
    .replace('\r', " ")
    .replace('\n', " ")
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");
  if preview.chars().count() > limit {
    preview = trim_buffer_to_recent(&preview, limit);
  }
  preview
}

fn is_prompt_echo_completion(value: &str) -> bool {
  let trimmed = value.trim();
  if trimmed.is_empty() {
    return false;
  }

  let lowered = trimmed.to_ascii_lowercase();
  let exact_bad = [
    "用户还没写完的后续内容",
    "只输出补全内容",
    "只返回补全内容",
    "直接输出续写",
  ];
  if exact_bad.iter().any(|bad| trimmed == *bad) {
    return true;
  }

  // Prompt field-name echoes from local /generate prompts.
  let prompt_markers = [
    "style:",
    "target_language",
    "last_input_is_boundary",
    "ime_hint",
    "full_context",
    "current_input",
    "current_sentence_tail",
    "recent_context",
    "输出要求",
    "任务：",
    "当前输入：",
    "当前句尾：",
    "最近上下文：",
  ];
  if prompt_markers.iter().any(|marker| lowered.starts_with(marker)) {
    return true;
  }
  let marker_hits = prompt_markers
    .iter()
    .filter(|marker| lowered.contains(**marker))
    .count();
  if marker_hits >= 2 {
    return true;
  }

  false
}

fn strip_reasoning_artifacts(raw: &str) -> String {
  let mut value = raw.to_string();

  // Some local models expose reasoning in <think>...</think>; keep only the final answer.
  if let Some(end_idx) = value.rfind("</think>") {
    let tail = value[end_idx + "</think>".len()..].trim();
    if !tail.is_empty() {
      return tail.to_string();
    }
  }
  if let (Some(start_idx), Some(end_rel_idx)) = (value.find("<think>"), value.find("</think>")) {
    let end_idx = end_rel_idx + "</think>".len();
    value.replace_range(start_idx..end_idx, "");
  }

  // Some local models include "Thinking... ...done thinking." traces.
  let lowered = value.to_ascii_lowercase();
  if let Some(done_idx) = lowered.rfind("done thinking.") {
    let tail = value[done_idx + "done thinking.".len()..].trim();
    if !tail.is_empty() {
      return tail.to_string();
    }
  }
  if lowered.starts_with("thinking...") {
    let lines: Vec<&str> = value.lines().collect();
    if let Some(last) = lines.last() {
      let candidate = last.trim();
      if !candidate.is_empty() {
        return candidate.to_string();
      }
    }
  }

  value
}

fn output_language_hint(language: OutputLanguage) -> &'static str {
  match language {
    OutputLanguage::Auto => "Follow the user's input language.",
    OutputLanguage::Zh => "简体中文（禁止英文与拼音）",
    OutputLanguage::En => "English.",
    OutputLanguage::Ja => "Japanese.",
    OutputLanguage::Ko => "Korean.",
    OutputLanguage::Es => "Spanish.",
    OutputLanguage::Fr => "French.",
    OutputLanguage::De => "German.",
  }
}

fn enforce_language_alignment(context: &str, completion: String, language: OutputLanguage) -> String {
  if completion.trim().is_empty() {
    return completion;
  }

  match language {
    OutputLanguage::Auto => {
      if contains_cjk(context) && !contains_cjk(&completion) {
        log::info!("completion dropped: language mismatch (auto/CJK context)");
        return String::new();
      }
    }
    OutputLanguage::Zh => {
      if !contains_cjk(&completion) {
        log::info!("completion dropped: language mismatch (expected zh)");
        return String::new();
      }
    }
    OutputLanguage::Ja => {
      if !contains_japanese_or_cjk(&completion) {
        log::info!("completion dropped: language mismatch (expected ja)");
        return String::new();
      }
    }
    OutputLanguage::Ko => {
      if !contains_hangul(&completion) {
        log::info!("completion dropped: language mismatch (expected ko)");
        return String::new();
      }
    }
    OutputLanguage::En | OutputLanguage::Es | OutputLanguage::Fr | OutputLanguage::De => {
      if !contains_latin_letter(&completion) {
        log::info!("completion dropped: language mismatch (expected latin language)");
        return String::new();
      }
    }
  }

  completion
}

fn contains_cjk(value: &str) -> bool {
  value.chars().any(|ch| {
    ('\u{3400}'..='\u{4DBF}').contains(&ch)
      || ('\u{4E00}'..='\u{9FFF}').contains(&ch)
      || ('\u{F900}'..='\u{FAFF}').contains(&ch)
      || ('\u{3040}'..='\u{30FF}').contains(&ch)
      || ('\u{AC00}'..='\u{D7AF}').contains(&ch)
  })
}

fn contains_japanese_or_cjk(value: &str) -> bool {
  value.chars().any(|ch| {
    ('\u{3040}'..='\u{30FF}').contains(&ch)
      || ('\u{3400}'..='\u{4DBF}').contains(&ch)
      || ('\u{4E00}'..='\u{9FFF}').contains(&ch)
      || ('\u{F900}'..='\u{FAFF}').contains(&ch)
  })
}

fn contains_hangul(value: &str) -> bool {
  value.chars().any(|ch| ('\u{AC00}'..='\u{D7AF}').contains(&ch))
}

fn contains_latin_letter(value: &str) -> bool {
  value.chars().any(|ch| ch.is_ascii_alphabetic() || ('\u{00C0}'..='\u{024F}').contains(&ch))
}

fn tokenize_latin_words(value: &str) -> Vec<String> {
  value
    .split(|ch: char| !ch.is_ascii_alphanumeric())
    .map(|s| s.trim().to_ascii_lowercase())
    .filter(|s| s.len() >= 3)
    .collect()
}

fn enforce_topic_alignment(
  context: &str,
  recent_history: &str,
  full_context: &str,
  completion: String,
) -> String {
  let trimmed = completion.trim();
  if trimmed.is_empty() {
    return completion;
  }

  if contains_cjk(context) || contains_cjk(recent_history) {
    return completion;
  }

  let source_text = format!("{full_context} {recent_history} {context}");
  let source_words = tokenize_latin_words(&source_text);
  if source_words.is_empty() {
    return completion;
  }

  let completion_words = tokenize_latin_words(trimmed);
  if completion_words.is_empty() {
    return completion;
  }

  let overlap = completion_words
    .iter()
    .filter(|word| source_words.iter().any(|src| src == *word))
    .count();
  let ratio = overlap as f32 / completion_words.len() as f32;
  if ratio < 0.15 && completion_words.len() >= 4 {
    log::info!("completion dropped: low topic overlap ({ratio:.2})");
    return String::new();
  }

  completion
}

fn enforce_completion_quality(
  context: &str,
  recent_history: &str,
  full_context: &str,
  completion: String,
  language: OutputLanguage,
) -> String {
  let trimmed = completion.trim();
  if trimmed.is_empty() {
    return completion;
  }

  if contains_question_marker(trimmed) && !contains_question_marker(context) {
    log::info!("completion dropped: unexpected question pattern");
    return String::new();
  }

  if language == OutputLanguage::Zh {
    let sentence_tail = extract_current_sentence_tail(context);
    let pinyin_mode = looks_like_pinyin_tail(&sentence_tail);
    if pinyin_mode {
      // During pinyin composition, long semantic continuations are usually wrong.
      if trimmed.chars().count() > 28 {
        log::info!("completion dropped: too long during pinyin composition");
        return String::new();
      }
      if trimmed.chars().count() > 12
        && ["我是", "我刚刚", "当然", "好的", "可以"].iter().any(|head| trimmed.starts_with(head))
      {
        log::info!("completion dropped: generic opener during pinyin composition");
        return String::new();
      }
    }

    if contains_cjk(&sentence_tail) {
      let overlap = trimmed
        .chars()
        .filter(|ch| !ch.is_whitespace() && sentence_tail.contains(*ch))
        .count();
      if overlap == 0 && trimmed.chars().count() >= 6 {
        log::info!("completion dropped: low chinese overlap with sentence tail");
        return String::new();
      }
    } else if !pinyin_mode
      && !contains_cjk(recent_history)
      && !contains_cjk(full_context)
      && trimmed.chars().count() > 24
    {
      log::info!("completion dropped: low-confidence zh completion without CJK context");
      return String::new();
    }

    let source = format!("{full_context}{recent_history}{context}");
    let generic_slogan = ["让世界", "见证我们的", "我们的实力", "我们的辉煌", "梦想成真"];
    let has_generic_slogan = generic_slogan.iter().any(|phrase| trimmed.contains(phrase));
    if has_generic_slogan {
      let slogan_keywords = ["世界", "见证", "实力", "辉煌", "梦想"];
      let source_mentions = slogan_keywords.iter().any(|kw| source.contains(kw));
      if !source_mentions {
        log::info!("completion dropped: generic slogan without source evidence");
        return String::new();
      }
    }

    let source_has_cjk = contains_cjk(&source);
    if !source_has_cjk {
      // No committed Chinese context yet: still allow completion, but block slogan-like drift.
      let strong_generic = ["世界", "辉煌", "实力", "见证", "梦想", "未来", "成功", "伟大"];
      if strong_generic.iter().any(|kw| trimmed.contains(kw)) {
        log::info!("completion dropped: no-cjk context with generic slogan keyword");
        return String::new();
      }
    }
  }

  completion
}

fn normalize_candidate_for_rerank(
  context: &str,
  recent_history: &str,
  full_context: &str,
  language: OutputLanguage,
  raw: &str,
) -> String {
  if RELAX_COMPLETION_GUARDS {
    return sanitize_completion(context, raw);
  }
  let cleaned = sanitize_completion(context, raw);
  let cleaned = enforce_topic_alignment(context, recent_history, full_context, cleaned);
  let cleaned = enforce_language_alignment(context, cleaned, language);
  enforce_completion_quality(context, recent_history, full_context, cleaned, language)
}

fn cjk_overlap_count(a: &str, b: &str) -> usize {
  let set_a: HashSet<char> = a.chars().filter(|ch| is_cjk_char(*ch)).collect();
  let set_b: HashSet<char> = b.chars().filter(|ch| is_cjk_char(*ch)).collect();
  set_a.intersection(&set_b).count()
}

fn score_candidate(
  context: &str,
  recent_history: &str,
  full_context: &str,
  language: OutputLanguage,
  completion: &str,
) -> i32 {
  let mut score = 0i32;
  let char_len = completion.chars().count() as i32;
  let sentence_tail = extract_current_sentence_tail(context);
  let source_text = format!("{full_context} {recent_history} {context}");

  match language {
    OutputLanguage::Zh | OutputLanguage::Auto => {
      score += if (6..=60).contains(&char_len) {
        8
      } else if (61..=96).contains(&char_len) {
        3
      } else {
        -4
      };
      if contains_question_marker(completion) && !contains_question_marker(context) {
        score -= 6;
      }
      let overlap = cjk_overlap_count(&sentence_tail, completion) as i32;
      score += overlap.min(6) * 2;
      if looks_like_pinyin_tail(&sentence_tail) && contains_cjk(completion) {
        score += 4;
      }
      if ["我是", "我刚刚", "当然", "好的", "可以", "然后", "其实"]
        .iter()
        .any(|head| completion.starts_with(head))
      {
        score -= 4;
      }
    }
    OutputLanguage::Ja | OutputLanguage::Ko | OutputLanguage::En | OutputLanguage::Es | OutputLanguage::Fr | OutputLanguage::De => {
      let source_words = tokenize_latin_words(&source_text);
      let candidate_words = tokenize_latin_words(completion);
      if !candidate_words.is_empty() {
        let overlap = candidate_words
          .iter()
          .filter(|word| source_words.iter().any(|src| src == *word))
          .count() as i32;
        score += overlap * 2;
      }
      score += if (2..=24).contains(&char_len) { 5 } else { -1 };
    }
  }

  score
}

fn select_best_candidate(
  context: &str,
  recent_history: &str,
  full_context: &str,
  language: OutputLanguage,
  candidates: &[String],
) -> String {
  if RELAX_COMPLETION_GUARDS {
    let mut viable = 0usize;
    for raw in candidates {
      let cleaned = sanitize_completion(context, raw);
      if cleaned.trim().is_empty() {
        continue;
      }
      viable += 1;
      log::info!(
        "candidate rerank raw={} viable={} best_score=relaxed",
        candidates.len(),
        viable
      );
      return cleaned;
    }
    log::info!(
      "candidate rerank raw={} viable={} best_score=relaxed",
      candidates.len(),
      viable
    );
    return String::new();
  }

  let mut best = String::new();
  let mut best_score = i32::MIN;
  let mut viable = 0usize;

  for raw in candidates {
    let cleaned = normalize_candidate_for_rerank(context, recent_history, full_context, language, raw);
    if cleaned.trim().is_empty() {
      continue;
    }
    viable += 1;
    let score = score_candidate(context, recent_history, full_context, language, &cleaned);
    if score > best_score {
      best_score = score;
      best = cleaned;
    }
  }

  log::info!(
    "candidate rerank raw={} viable={} best_score={}",
    candidates.len(),
    viable,
    best_score
  );

  if best.trim().is_empty() {
    return String::new();
  }

  if candidates.len() == 1 {
    return best;
  }

  if best_score < 2 {
    return String::new();
  }
  best
}

fn setup_tray(app: &tauri::App<Wry>) -> tauri::Result<()> {
  let open_item = MenuItem::with_id(app, "open", "Open TypeAce", true, None::<&str>)?;
  let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
  let tray_menu = Menu::with_items(app, &[&open_item, &quit_item])?;

  TrayIconBuilder::with_id("typeace-tray")
    .menu(&tray_menu)
    .show_menu_on_left_click(false)
    .on_menu_event(|app, event| match event.id.as_ref() {
      "open" => {
        if let Some(window) = app.get_webview_window("main") {
          let _ = window.show();
          let _ = window.set_focus();
        }
      }
      "quit" => app.exit(0),
      _ => {}
    })
    .on_tray_icon_event(|tray, event| {
      if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
      } = event
      {
        let app = tray.app_handle();
        if let Some(window) = app.get_webview_window("main") {
          let is_visible = window.is_visible().unwrap_or(false);
          if is_visible {
            let _ = window.hide();
          } else {
            let _ = window.show();
            let _ = window.set_focus();
          }
        }
      }
    })
    .build(app)?;

  Ok(())
}

fn setup_main_window_close_behavior(app: &AppHandle) {
  if let Some(window) = app.get_webview_window("main") {
    let win = window.clone();
    window.on_window_event(move |event| {
      if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = win.hide();
      }
    });
  }
}

#[cfg(target_os = "windows")]
fn set_launch_on_startup(enabled: bool) -> Result<(), String> {
  use winreg::enums::HKEY_CURRENT_USER;
  use winreg::RegKey;

  let hkcu = RegKey::predef(HKEY_CURRENT_USER);
  let (run, _) = hkcu
    .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
    .map_err(|e| format!("registry open failed: {e}"))?;

  if enabled {
    let exe = std::env::current_exe().map_err(|e| format!("current exe lookup failed: {e}"))?;
    let command = format!("\"{}\" --autostart", exe.display());
    run
      .set_value("TypeAce", &command)
      .map_err(|e| format!("registry write failed: {e}"))?;
  } else {
    let _ = run.delete_value("TypeAce");
  }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
fn set_launch_on_startup(_enabled: bool) -> Result<(), String> {
  Ok(())
}

#[cfg(target_os = "windows")]
fn is_launch_on_startup() -> bool {
  use winreg::enums::HKEY_CURRENT_USER;
  use winreg::RegKey;

  let hkcu = RegKey::predef(HKEY_CURRENT_USER);
  let Ok(run) = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") else {
    return false;
  };
  run.get_value::<String, _>("TypeAce").is_ok()
}

#[cfg(not(target_os = "windows"))]
fn is_launch_on_startup() -> bool {
  false
}

fn today_key() -> String {
  Local::now().format("%Y-%m-%d").to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(
      tauri_plugin_log::Builder::default()
        .level(log::LevelFilter::Info)
        .build(),
    )
    .setup(|app| {
      setup_tray(app)?;

      let app_handle = app.handle().clone();
      setup_main_window_close_behavior(&app_handle);
      ensure_ghost_window(&app_handle)
        .map_err(|err| tauri::Error::Anyhow(anyhow::anyhow!(err)))?;

      let mut persisted = load_persisted_state(&app_handle);
      persisted.settings = normalize_settings(persisted.settings);
      persisted.settings.autostart = is_launch_on_startup();
      persisted.usage.refresh_day();

      let runtime = RuntimeState {
        settings: persisted.settings.clone(),
        usage: persisted.usage.clone(),
        buffer: String::new(),
        session_context: String::new(),
        history: Vec::new(),
        prediction_seq: 0,
        last_input_was_boundary: false,
        suggestion: None,
        ghost_visible: false,
        pending_tab_accept: false,
        pending_task: None,
      };

      let managed = Arc::new(ManagedState {
        runtime: Arc::new(Mutex::new(runtime)),
        ai_client: Client::new(),
      });
      app.manage(managed.clone());

      if let Ok(runtime) = managed.runtime.lock() {
        let _ = persist_state(&app_handle, &runtime.settings, &runtime.usage);
      }

      let (tx, rx) = mpsc::unbounded_channel();
      start_global_listener(tx);
      tauri::async_runtime::spawn(process_input_events(app_handle.clone(), managed.clone(), rx));
      tauri::async_runtime::spawn(run_ghost_follow_loop(app_handle.clone(), managed.clone()));

      if std::env::args().any(|arg| arg == "--autostart") {
        if let Some(window) = app_handle.get_webview_window("main") {
          let _ = window.hide();
        }
      }

      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_app_state,
      update_settings,
      dismiss_ghost,
      accept_suggestion,
      clear_usage
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

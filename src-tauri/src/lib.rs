
use std::{
  fs,
  path::PathBuf,
  sync::{Arc, Mutex},
  time::Duration,
};

use arboard::Clipboard;
use chrono::Local;
use reqwest::Client;
use rdev::{listen, simulate, Event, EventType, Key};
use serde::{Deserialize, Serialize};
use tauri::{
  menu::{Menu, MenuItem},
  tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
  AppHandle, Emitter, Manager, PhysicalPosition, Position, State, WindowEvent, Wry,
};
use tokio::sync::mpsc;

const FREE_DAILY_LIMIT: u32 = 50;
const DEFAULT_MIN_TRIGGER_LEN: usize = 10;
const MAX_CONTEXT_CHARS: usize = 300;
const STATE_FILENAME: &str = "typeace-state.json";
const GHOST_WIDTH: i32 = 720;
const GHOST_HEIGHT: i32 = 26;
const GHOST_OFFSET_X: i32 = 1;
const GHOST_OFFSET_Y: i32 = 0;
const WINDOW_FALLBACK_X: i32 = 28;
const WINDOW_FALLBACK_Y: i32 = 48;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
  enabled: bool,
  trigger_delay_ms: u64,
  ai_style: AiStyle,
  hotkey_mode: HotkeyMode,
  custom_hotkey: String,
  is_pro: bool,
  autostart: bool,
  minimum_length: usize,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      enabled: true,
      trigger_delay_ms: 500,
      ai_style: AiStyle::Casual,
      hotkey_mode: HotkeyMode::Tab,
      custom_hotkey: "Ctrl+Shift+Space".to_string(),
      is_pro: false,
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

  fn remaining(&self, is_pro: bool) -> Option<u32> {
    if is_pro {
      None
    } else {
      Some(self.free_limit.saturating_sub(self.used_today))
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
      remaining_today: self.usage.remaining(self.settings.is_pro),
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
struct ChatCompletionRequest {
  model: String,
  messages: Vec<ChatMessage>,
  temperature: f32,
  max_tokens: u16,
  top_p: f32,
  stream: bool,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
  role: String,
  content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
  choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
  message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
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
  settings.trigger_delay_ms = settings.trigger_delay_ms.clamp(200, 800);
  settings.minimum_length = settings.minimum_length.max(DEFAULT_MIN_TRIGGER_LEN);
  settings
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

    if !runtime.settings.is_pro && runtime.usage.used_today >= runtime.usage.free_limit {
      runtime.suggestion = None;
      runtime.ghost_visible = false;
      runtime.pending_tab_accept = false;
      hide_ghost_window(app);
      return Err(format!(
        "Free 版今日配额已用完（{} 次）。",
        runtime.usage.free_limit
      ));
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
    if !runtime.settings.is_pro {
      runtime.usage.used_today = runtime.usage.used_today.saturating_add(1);
    }
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

fn caret_position() -> Option<(i32, i32)> {
  #[cfg(target_os = "windows")]
  {
    ghost_anchor_position_windows()
  }

  #[cfg(not(target_os = "windows"))]
  {
    None
  }
}

#[cfg(target_os = "windows")]
fn ghost_anchor_position_windows() -> Option<(i32, i32)> {
  if let Some(pos) = caret_position_windows() {
    return Some(pos);
  }

  if let Some(pos) = mouse_position_windows() {
    log::info!("ghost anchor fallback: mouse position");
    return Some(pos);
  }

  if let Some(pos) = foreground_window_fallback_windows() {
    log::info!("ghost anchor fallback: foreground window position");
    return Some(pos);
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

#[cfg(target_os = "windows")]
fn foreground_window_fallback_windows() -> Option<(i32, i32)> {
  use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};

  unsafe {
    let foreground = GetForegroundWindow();
    if foreground.0.is_null() {
      return None;
    }

    let mut rect = windows::Win32::Foundation::RECT::default();
    if GetWindowRect(foreground, &mut rect).is_err() {
      return None;
    }

    Some((rect.left + WINDOW_FALLBACK_X, rect.top + WINDOW_FALLBACK_Y))
  }
}

fn ensure_ghost_window(app: &AppHandle) -> Result<(), String> {
  if app.get_webview_window("ghost").is_some() {
    return Ok(());
  }

  tauri::WebviewWindowBuilder::new(app, "ghost", tauri::WebviewUrl::App("ghost".into()))
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
    .inner_size(GHOST_WIDTH as f64, GHOST_HEIGHT as f64)
    .build()
    .map_err(|e| format!("failed creating ghost window: {e}"))?;

  Ok(())
}

fn show_ghost_window(app: &AppHandle, text: &str) {
  if let Err(err) = ensure_ghost_window(app) {
    emit_error_event(app, err);
    return;
  }

  if let Some(window) = app.get_webview_window("ghost") {
    let Some((x, y)) = caret_position() else {
      log::info!("show_ghost_window skipped: position unavailable");
      let _ = window.hide();
      return;
    };

    let _ = window.set_position(Position::Physical(PhysicalPosition::new(
      x + GHOST_OFFSET_X,
      y + GHOST_OFFSET_Y,
    )));

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

fn schedule_prediction(app: AppHandle, managed: Arc<ManagedState>, context: String) {
  let (delay_ms, settings_snapshot) = {
    let mut runtime = match managed.runtime.lock() {
      Ok(value) => value,
      Err(_) => return,
    };
    cancel_pending_locked(&mut runtime);
    (
      runtime.settings.trigger_delay_ms.clamp(200, 800),
      runtime.settings.clone(),
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
      if !runtime.settings.enabled {
        log::info!("schedule_prediction skipped: disabled");
        return;
      }
      if runtime.buffer != context {
        log::info!("schedule_prediction skipped: context changed");
        return;
      }
      if runtime.buffer.chars().count() < runtime.settings.minimum_length {
        log::info!("schedule_prediction skipped: below min length");
        return;
      }
    }

    log::info!("request_prediction with {} chars", context.chars().count());
    let response = request_prediction(&managed_for_task.ai_client, &settings_snapshot, &context).await;
    let prediction = match response {
      Ok(value) => value,
      Err(err) => {
        log::error!("request_prediction failed: {}", err);
        emit_error_event(&app_for_task, err);
        return;
      }
    };

    if prediction.trim().is_empty() {
      log::info!("request_prediction returned empty");
      return;
    }

    {
      let mut runtime = match managed_for_task.runtime.lock() {
        Ok(value) => value,
        Err(_) => return,
      };
      if runtime.buffer != context || !runtime.settings.enabled {
        log::info!("prediction dropped: state changed");
        return;
      }

      runtime.usage.refresh_day();
      if !runtime.settings.is_pro && runtime.usage.used_today >= runtime.usage.free_limit {
        log::info!("prediction dropped: quota reached");
        return;
      }

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
    runtime.buffer = trim_buffer_to_recent(&runtime.buffer, MAX_CONTEXT_CHARS);
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

        if matches!(key, Key::Return | Key::Tab) {
          log::info!("global key: Return/Tab");
          let mut runtime = match managed.runtime.lock() {
            Ok(value) => value,
            Err(_) => continue,
          };
          runtime.buffer.clear();
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

async fn request_prediction(client: &Client, settings: &Settings, context: &str) -> Result<String, String> {
  let mock_enabled = std::env::var("TYPEACE_MOCK_COMPLETION")
    .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
    .unwrap_or(false);
  log::info!("request_prediction mock_enabled={}", mock_enabled);
  if mock_enabled {
    return Ok("work together on this project".to_string());
  }

  let api_key = match std::env::var("OPENAI_API_KEY") {
    Ok(value) => value,
    Err(_) => {
      if cfg!(debug_assertions) {
        log::warn!("OPENAI_API_KEY missing in debug mode, using mock completion fallback");
        return Ok("work together on this project".to_string());
      }
      return Err("OPENAI_API_KEY 未配置".to_string());
    }
  };

  let model = if settings.is_pro { "gpt-4.1-mini" } else { "gpt-4o-mini" };
  let style_hint = match settings.ai_style {
    AiStyle::Casual => "Continue naturally in a casual tone.",
    AiStyle::Professional => "Continue in a concise professional tone.",
    AiStyle::Creative => "Continue with an imaginative but readable tone.",
  };

  let payload = ChatCompletionRequest {
    model: model.to_string(),
    messages: vec![
      ChatMessage {
        role: "system".to_string(),
        content: "You are an inline typing completion engine. Return only the exact continuation text with no quotes, no markdown, no explanations, and no prefix.".to_string(),
      },
      ChatMessage {
        role: "user".to_string(),
        content: format!(
          "Style: {style_hint}\nContinue this user input only:\n{context}\n\nOutput requirement: continuation only."
        ),
      },
    ],
    temperature: if matches!(settings.ai_style, AiStyle::Creative) {
      0.9
    } else {
      0.5
    },
    max_tokens: 64,
    top_p: 1.0,
    stream: false,
  };

  let endpoint =
    std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string());
  let response = client
    .post(endpoint)
    .bearer_auth(api_key)
    .json(&payload)
    .timeout(Duration::from_secs(10))
    .send()
    .await
    .map_err(|e| format!("AI request failed: {e}"))?;

  if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_else(|_| String::new());
    return Err(format!("AI request rejected ({status}): {body}"));
  }

  let data = response
    .json::<ChatCompletionResponse>()
    .await
    .map_err(|e| format!("invalid AI response: {e}"))?;

  let raw = data
    .choices
    .first()
    .map(|choice| choice.message.content.clone())
    .unwrap_or_default();

  let cleaned = sanitize_completion(context, &raw);
  Ok(cleaned)
}

fn sanitize_completion(context: &str, raw: &str) -> String {
  let mut value = raw.trim().replace('\n', " ");
  if value.starts_with(context) {
    value = value[context.len()..].trim_start().to_string();
  }
  value = value.trim_matches('"').to_string();
  trim_buffer_to_recent(&value, 120)
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

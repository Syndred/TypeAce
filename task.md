# TypeAce Task Progress

Last updated: 2026-03-01

## Overall

- Status: `In Progress (MVP mostly done)`
- Platform: `Windows desktop (Tauri 2 + Next.js 15)`
- Build status:
  - `npm run lint` -> pass
  - `npm run build` -> pass
  - `cargo check --manifest-path src-tauri/Cargo.toml` -> pass

## Task Checklist

### Task 1: Project Initialization

- [x] Tauri 2.0 project created
- [x] Next.js 15 + TypeScript ready
- [x] Tailwind CSS v4 ready
- [x] shadcn/ui integrated
- [x] Main window set to `400x600`
- [x] Background running (close to tray)
- [x] Startup option (Windows registry Run key)

Notes:
- Config: `src-tauri/tauri.conf.json`
- Tray and close-to-background behavior: `src-tauri/src/lib.rs`

### Task 2: Frontend Settings UI

- [x] Global enable/disable switch
- [x] Trigger delay slider `200-800ms`
- [x] AI style selector `Casual/Professional/Creative`
- [x] Hotkey config `Tab/Ctrl+Space/Custom`
- [x] Free/Pro status display
- [x] Daily usage stats display

Files:
- `src/app/page.tsx`
- `src/lib/typeace.ts`
- `src/lib/tauri-client.ts`

### Task 3: Rust Global Keyboard Listening

- [x] Global keyboard listener (`rdev`)
- [x] Input context accumulation from typed text events
- [x] Password-like control exclusion (Windows `ES_PASSWORD`)
- [x] Sensitive content policy: no active app text scraping API, only runtime typed characters
- [x] Only normal text path is tracked

Files:
- `src-tauri/src/lib.rs`

### Task 4: Pause Detection

- [x] Trigger AI after `500ms` default pause
- [x] Cancel pending request immediately when typing continues
- [x] Minimum trigger length `>= 10`
- [x] Non-blocking async behavior to avoid typing lag

Files:
- `src-tauri/src/lib.rs`

### Task 5: AI Prediction API

- [x] Cloud model path for Free: `gpt-4o-mini`
- [x] Context input -> continuation output
- [x] Output constrained to plain continuation text
- [x] Example behavior supported (`I think that we should` -> continuation)

Files:
- `src-tauri/src/lib.rs`

Notes:
- Requires `OPENAI_API_KEY` for real cloud calls.
- Dev mode currently contains a fallback mock when key is missing, to keep local testing unblocked.

### Task 6: Ghost Text Display

- [x] Grey ghost text overlay near caret
- [x] No focus stealing from main typing window
- [x] Original text not overwritten
- [x] Auto-hide on continued typing
- [x] Hide on `Esc`

Files:
- `src/app/ghost/page.tsx`
- `src-tauri/src/lib.rs`

### Task 7: Tab One-Key Completion

- [x] Accept suggestion on `Tab` (or configured hotkey)
- [x] System paste simulation (`Ctrl+V`) for cross-app insertion
- [x] Designed for global apps (WeChat/Browser/Notion etc.)
- [x] Runtime stability verified by repeated dev tests

Files:
- `src-tauri/src/lib.rs`

### Task 8: Free vs Pro Limits

- [x] Free: daily `50` completions
- [x] Free: basic model
- [x] Pro: unlimited usage
- [x] Pro: advanced model path and style controls

Files:
- `src-tauri/src/lib.rs`
- `src/app/page.tsx`

## Remaining / Follow-up

- [ ] Production cleanup: remove or gate extra debug logs in `src-tauri/src/lib.rs`
- [ ] Optional: disable dev fallback mock before release if strict cloud-only behavior is required
- [ ] Optional: add end-to-end test harness for acceptance in external apps

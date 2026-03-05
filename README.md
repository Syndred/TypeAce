# TypeAce

TypeAce is a `Tauri 2 + Next.js 15` desktop typing assistant with:

- global keyboard listening
- ghost text preview near the caret
- Tab / Ctrl+Space / custom hotkey accept
- local-only AI completion via Ollama (no cloud API key)

## Local-Only Architecture

TypeAce now uses only local model inference:

- Rust backend sends completion requests to local Ollama (`/api/generate`)
- model switching by language:
  - CJK-oriented text -> `localModelZh`
  - Latin-oriented text -> `localModelEn`
- no OpenAI / DeepSeek API fields in settings UI

## Prerequisites (Windows)

1. Node.js 22+
2. Rust stable (via rustup)
3. Visual Studio Build Tools 2022 (`MSVC v143` + `Windows SDK`)
4. WebView2 Runtime
5. Ollama installed and running locally

## Install

```bash
npm install
```

## Pull Local Models

```bash
ollama pull qwen2.5:1.5b
ollama pull llama3.2:1b
```

You can change model names in the TypeAce settings page.

## Run

Frontend only:

```bash
npm run dev
```

Desktop app:

```bash
npm run tauri:dev
```

Build:

```bash
npm run build
npm run tauri:build
```

## Notes

- Default local endpoint is `http://127.0.0.1:11434/api/generate`.
- During pinyin composition, TypeAce waits for commit boundary (for example space/punctuation) before querying to avoid low-quality interim guesses.

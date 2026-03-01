# TypeAce

TypeAce 是一个基于 `Tauri 2 + Next.js 15` 的全局 AI 打字补全工具原型，包含：

- 全局监听键盘输入（仅普通文本、排除密码输入场景）
- 输入暂停触发 AI 预测（可配置 200-800ms，默认 500ms）
- Ghost Text 透明浮层预览
- Tab / Ctrl+Space / 自定义快捷键补全
- Free / Pro 限制（免费每日 50 次）
- 后台托盘运行 + Windows 开机自启

## 目录结构

```text
.
|- src/                         # Next.js 前端
|  |- app/page.tsx              # 设置界面
|  |- app/ghost/page.tsx        # Ghost 文本浮层页面
|  |- app/layout.tsx
|  |- app/globals.css
|  |- components/ui/*           # shadcn/ui 组件
|  |- lib/tauri-client.ts       # Tauri invoke/event 封装
|  |- lib/typeace.ts            # 前端类型定义
|- src-tauri/                   # Tauri + Rust 后端
|  |- src/lib.rs                # 核心逻辑（监听/AI/补全/配额/托盘）
|  |- src/main.rs
|  |- tauri.conf.json
|  |- Cargo.toml
|  |- capabilities/default.json
|- .cargo/config.toml           # Cargo 镜像配置（rsproxy）
|- package.json
|- next.config.ts
```

## 环境要求（Windows）

1. Node.js 22+
2. Rust stable（推荐通过 rustup）
3. **Visual Studio Build Tools 2022（必须）**
   - 需要包含 `MSVC v143` / `Windows SDK` / `C++ build tools`
4. WebView2 Runtime（通常系统已带）

## 安装依赖

```bash
npm install
```

## 配置 OpenAI Key

运行前在系统环境变量中配置：

```bash
OPENAI_API_KEY=你的密钥
```

可选：

```bash
OPENAI_BASE_URL=https://api.openai.com/v1/chat/completions
```

## 开发运行

仅前端：

```bash
npm run dev
```

完整桌面应用（Tauri）：

```bash
npm run tauri:dev
```

## 打包构建

```bash
npm run build
npm run tauri:build
```

## 已完成任务对应

1. 项目初始化：`Tauri 2 + Next 15 + TS + Tailwind v4 + shadcn/ui`
2. 设置界面：总开关、延迟、风格、快捷键、套餐状态、使用次数
3. Rust 全局监听：仅普通文本输入，密码输入场景排除
4. 输入暂停检测：500ms 默认触发、继续输入取消、最小长度 10
5. AI 接口：`gpt-4o-mini`（Free）/ `gpt-4.1-mini`（Pro）
6. Ghost Text：透明窗口显示，不抢主窗口焦点
7. Tab 补全：系统级粘贴注入（Ctrl+V 模拟）
8. Free / Pro 限制：每日 50 次 / Pro 无限

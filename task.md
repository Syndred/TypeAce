# TypeAce 任务进度（实时）

最后更新：2026-03-01

## 总览

- 当前状态：`进行中（核心功能已打通，正在修复 Tab 真实注入稳定性）`
- 目标平台：`Windows 桌面端（Tauri 2 + Next.js 15）`
- 构建状态：
  - `npm run lint`：通过
  - `npm run build`：通过
  - `cargo check --manifest-path src-tauri/Cargo.toml`：通过

## 分任务清单

### 任务 1：项目初始化

- [x] 创建 `Tauri 2.0` 项目
- [x] 集成 `Next.js 15 + TypeScript`
- [x] 集成 `Tailwind CSS v4`
- [x] 集成 `shadcn/ui`
- [x] 主窗口尺寸设置为 `400x600`
- [x] 支持后台运行（关闭主窗体后最小化到托盘）
- [x] 支持开机自启（Windows 注册表 `Run`）

关联文件：
- `src-tauri/tauri.conf.json`
- `src-tauri/src/lib.rs`

### 任务 2：前端设置界面

- [x] 总开关：启用 / 禁用 TypeAce
- [x] 触发延迟：`200-800ms` 滑动条
- [x] AI 风格：`Casual / Professional / Creative`
- [x] 快捷键：`Tab / Ctrl+Space / 自定义`
- [x] 免费版 / Pro 状态显示
- [x] 每日使用统计显示

关联文件：
- `src/app/page.tsx`
- `src/lib/typeace.ts`
- `src/lib/tauri-client.ts`

### 任务 3：Rust 全局键盘监听

- [x] 全局键盘监听（`rdev`）
- [x] 根据按键事件累积输入上下文
- [x] 排除密码输入框（Windows `ES_PASSWORD`）
- [x] 不抓取其他应用完整文本，仅处理运行期键入字符
- [x] 仅在普通文本输入控件中生效

关联文件：
- `src-tauri/src/lib.rs`

### 任务 4：输入暂停检测

- [x] 默认输入停止 `500ms` 触发预测
- [x] 用户继续输入立即取消上次请求
- [x] 最小触发长度 `>= 10`
- [x] 异步处理，不阻塞打字

关联文件：
- `src-tauri/src/lib.rs`

### 任务 5：AI 预测接口

- [x] 免费版模型：`gpt-4o-mini`
- [x] 上下文输入 -> 预测后文输出
- [x] 返回纯文本续写内容
- [x] 支持示例行为（`I think that we should` -> `work together on this project`）

关联文件：
- `src-tauri/src/lib.rs`

说明：
- 配置 `OPENAI_API_KEY` 后走真实云端请求。
- 开发模式未配置密钥时，当前会返回 mock 预测，便于本地联调。

### 任务 6：幽灵文本（Ghost Text）

- [x] 光标附近灰色提示展示
- [x] 不覆盖原输入内容
- [x] 继续输入自动消失
- [x] 按 `Esc` 消失
- [x] 已增加窗体非聚焦配置，避免抢焦点

关联文件：
- `src/app/ghost/page.tsx`
- `src-tauri/src/lib.rs`

### 任务 7：Tab 一键补全

- [x] 支持按热键接受补全（Tab / Ctrl+Space / 自定义）
- [x] 通过系统剪贴板 + 粘贴快捷键实现跨应用注入
- [x] 已加入 Windows `SendInput` 路径，并保留 `rdev` 兜底
- [x] 已改为 `Tab` 松开后执行补全，降低注入失败概率
- [ ] 待完成人工全链路复测（记事本、浏览器输入框、微信输入框）

关联文件：
- `src-tauri/src/lib.rs`

### 任务 8：免费版 / Pro 版限制

- [x] 免费版：每日 `50` 次补全
- [x] 免费版：基础模型
- [x] Pro 版：无限次
- [x] Pro 版：高级模型 / 风格路径

关联文件：
- `src-tauri/src/lib.rs`
- `src/app/page.tsx`

## 后续待办

- [ ] 完成 `Tab` 跨应用真实注入回归（当前正在处理）
- [ ] 发布前下调或收敛调试日志（`src-tauri/src/lib.rs`）
- [ ] 视发布策略决定是否保留无密钥 mock 回退

## 开发执行指令（按你要求保留）

请按照上面 8 个任务，完整实现 TypeAce 项目，  
一步一步输出代码，  
确保能直接编译、运行、在全系统实现 Tab 自动补全。

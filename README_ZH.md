<div align="center">

# CC Switch

### Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build、OpenCode、OpenClaw 和 Hermes Agent 的全方位管理工具

[![Version](https://img.shields.io/github/v/release/jiugjk/cc-switch?color=blue&label=version)](https://github.com/jiugjk/cc-switch/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20x64-lightgrey.svg)](https://github.com/jiugjk/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![Downloads](https://img.shields.io/github/downloads/jiugjk/cc-switch/total)](https://github.com/jiugjk/cc-switch/releases/latest)

<a href="https://trendshift.io/repositories/15372" target="_blank"><img src="https://trendshift.io/api/badge/repositories/15372" alt="farion1231%2Fcc-switch | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>
<a href="https://www.star-history.com/#jiugjk/cc-switch&Date"><picture><source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/badge?repo=farion1231/cc-switch&theme=dark" /><img alt="Star History Rank" src="https://api.star-history.com/badge?repo=farion1231/cc-switch" width="196" height="55" /></picture></a>

### 🌐 唯一官方网站：**[ccswitch.io](https://ccswitch.io)**

[English](README.md) | 中文 | [更新日志](CHANGELOG.md)

</div>

## 🔀 关于本 Fork

本仓库 fork 自 [farion1231/cc-switch](https://github.com/farion1231/cc-switch)，在上游 v3.19.1 基础上新增以下功能：

### CodexCont 与代理

- **CodexCont 推理自动续写** — `设置 → 路由 → CodexCont` 开关，在原生 `/v1/responses` 链上续写被截断的推理。仅在不会触发 Responses→Chat/Anthropic 转换时启用；完全复用 `RequestForwarder::forward_with_retry`（不绕过、不锁定供应商）。携带工具调用的轮次不会被吞掉；保留 legacy 流式 `function_call`。
- **路由可观测** — `ProxyStatus` 暴露 last-route 字段；通用设置页展示 `ProxyStatusSummary`（当前供应商、最近成功/失败、故障转移原因）。
- **额度归因** — 识别额度耗尽类错误并记录 `last_fallback_reason`；可选 `decouple_official_quota` 避免官方鉴权强行切到自定义 API 故障转移。**不新增**独立的「额度 fallback」开关，统一走既有 failover。
- **供应商能力解析** — 声明/启发式能力快照（含 Chat = 降级续写），便于更安全的路由决策。

### 应用探测与界面

- **顶部应用切换栏** — 自动隐藏未安装工具；窗口重新获得焦点时重探测；修复非全屏宽度下右侧按钮被挤出屏幕。
- **Codex 桌面** — Windows 上检测微软商店 `OpenAI.Codex` 包；端点提示随所选上游 API 格式联动。
- **检测文案本地化** — `not installed or not executable` 按当前界面语言显示。
- **工具官网链接** — 工具名旁快捷打开官网。
- **更新 Windows 安装命令** — 各工具一键安装命令已刷新。

### 构建

- **免费 Windows 自动构建** — GitHub Actions 在 `main` 的 **CI 通过后**（或在 `main` 上手动 `workflow_dispatch`）于免费托管 runner 上构建**未签名** NSIS / MSI / 便携版。本 fork **不发布** macOS/Linux 安装包，也无代码签名 / 自动更新通道。

用户侧开放确认：代理开/关的真实请求矩阵；接管开启时确认 ChatGPT 桌面流量进入 `127.0.0.1:15721` `/v1/responses`。

## 为什么选择 CC Switch？

现代 AI 编程依赖于 Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build、OpenCode、OpenClaw 和 Hermes 等工具——但每个工具都有自己的配置格式。切换 API 供应商意味着手动编辑 JSON、TOML 或 `.env` 文件，而在多个工具之间缺乏一个统一管理 MCP, SKILLS 的方式。

**CC Switch** 为你提供一个桌面应用来管理所有支持的 AI 工具。无需手动编辑配置文件，你将获得一个可视化界面，一键将供应商导入应用，一键在不同的供应商之间进行切换，内置 50+ 供应商预设、统一的 MCP, SKILLS 管理以及系统托盘即时切换功能——所有操作都基于可靠的 SQLite 数据库和原子写入机制，保护你的配置不被损坏。

- **一个应用，八个工具** — 在单一界面中管理 Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build、OpenCode、OpenClaw 和 Hermes
- **告别手动编辑** — 50+ 供应商预设，包括 AWS Bedrock、NVIDIA NIM 和社区中转服务；一键即可切换
- **统一 MCP, SKILLS 管理** — 一个面板管理 Claude、Codex、Gemini、Grok Build、OpenCode 和 Hermes 的 MCP, SKILLS, 支持双向同步
- **系统托盘快速切换** — 从托盘菜单即时切换供应商，无需打开完整应用
- **云同步** — 通过 Dropbox、OneDrive、iCloud 或 WebDAV 服务器在不同设备之间同步供应商数据
- **跨平台（上游）** — 上游支持 Windows、macOS 和 Linux；**本 fork 已发布安装包仅为 Windows x64**
- **小工具** - 内置了多种小工具来解决首次安装登录确认、禁止签名、插件拓展同步等多种功能

## 界面预览

|                  主界面                   |                  添加供应商                  |
| :---------------------------------------: | :------------------------------------------: |
| ![主界面](assets/screenshots/main-zh.png) | ![添加供应商](assets/screenshots/add-zh.png) |

## 功能特性

[完整更新日志](CHANGELOG.md) | [发布说明](docs/release-notes/v3.19.1-zh.md)

### 供应商管理

- **8 个支持工具，50+ 预设** — Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build、OpenCode、OpenClaw、Hermes；复制 key 即可一键导入
- **通用供应商** — 一份配置同步到 Claude Code、Codex 和 Gemini CLI
- 一键切换、系统托盘快速访问、拖拽排序、导入导出

### 代理与故障转移

- **本地代理热切换** — 格式转换、自动故障转移、熔断器、供应商健康监控和整流器
- **应用级代理接管** — 独立为 Claude、Codex、Gemini 或 Grok Build 配置代理，具体到单个供应商

### MCP、Prompts 与 Skills

- **统一 MCP 面板** — 管理 Claude、Codex、Gemini、Grok Build、OpenCode 和 Hermes 的 MCP 服务器，双向同步，支持 Deep Link 导入
- **Prompts** — Markdown 编辑器，跨应用同步（CLAUDE.md / AGENTS.md / GEMINI.md），回填保护
- **Skills** — 从 GitHub 仓库或 ZIP 文件一键安装，自定义仓库管理，支持软连接和文件复制

### 用量与成本追踪

- **用量仪表盘** — 跨供应商追踪支出、请求数和 Token 用量，趋势图表、详细请求日志和自定义模型定价

### 会话管理器与工作区

- 浏览、搜索和恢复支持的会话来源
- **工作区编辑器**（OpenClaw）— 编辑 Agent 文件（AGENTS.md、SOUL.md 等），支持 Markdown 预览

### 系统与平台

- **云同步** — 自定义配置目录（Dropbox、OneDrive、iCloud、坚果云、NAS）及 WebDAV 服务器同步
- **Deep Link** (`ccswitch://`) — 通过 URL 一键导入供应商、MCP 服务器、提示词和技能
- 深色 / 浅色 / 跟随系统主题、开机自启、原子写入、自动备份、国际化（简中/繁中/英/日）

## 常见问题

<details>
<summary><strong>CC Switch 支持哪些 AI 工具？</strong></summary>

CC Switch 支持八个工具：**Claude Code**、**Claude Desktop**、**Codex**、**Gemini CLI**、**Grok Build**、**OpenCode**、**OpenClaw** 和 **Hermes**。每个工具都有专属的供应商预设和配置管理。

</details>

<details>
<summary><strong>切换供应商后需要重启终端吗？</strong></summary>

大多数工具需要重启终端或 CLI 工具才能使更改生效。例外的是 **Claude Code**，它目前支持供应商数据的热切换，无需重启。

</details>

<details>
<summary><strong>切换供应商之后我的插件配置怎么不见了？</strong></summary>

CC Switch 使用“通用配置片段”功能，在不同的供应商之间传递 Key 和请求地址之外的通用数据，您可以在“编辑供应商”菜单的“通用配置面板”里，点击“从当前供应商提取”，把所有的通用数据提取到通用配置中，之后在新建“供应商”的时候，只要勾选“应用通用配置”（默认勾选），就会把插件等数据写入到新的供应商配置中。您的所有配置项都会保存在运行本软件的时候，第一次导入的默认供应商里面，不会丢失。

</details>

<details>
<summary><strong>macOS 安装</strong></summary>

CC Switch macOS 版本已通过 Apple 代码签名和公证，可直接下载安装，无需额外操作。推荐使用 `.dmg` 安装包。

</details>

<details>
<summary><strong>为什么总有一个正在激活中的供应商无法删除？</strong></summary>

本软件的设计原则是“最小侵入性”，即使卸载本软件，也不会影响应用的正常使用。

所以系统总会保留一个正在激活中的配置，因为如果将所有配置全部删除，该应用将无法正常使用。如果你不经常使用某个对应的应用，可以在设置中关掉该应用的显示。如果你想切换回官方登录，可以参考下条。

</details>

<details>
<summary><strong>如何切换回官方登录？</strong></summary>

可以在预设供应商里面添加一个官方供应商。切换过去之后，执行一遍 Log out / Log in 流程，之后便可以在官方供应商和第三方供应商之间随意切换。CodeX 可以在不同官方供应商之间进行切换，方便多个 Plus 或者 Team 账号之间切换。

</details>

<details>
<summary><strong>我的数据存储在哪里？</strong></summary>

- **数据库**：`~/.cc-switch/cc-switch.db`（SQLite — 供应商、MCP、提示词、技能）
- **本地设置**：`~/.cc-switch/settings.json`（设备级 UI 偏好设置）
- **备份**：`~/.cc-switch/backups/`（自动轮换，保留最近 10 个）
- **SKILLS**：`~/.cc-switch/skills/`（默认通过软链接连接到对应应用）
- **技能备份**：`~/.cc-switch/skill-backups/`（卸载前自动创建，保留最近 20 个）

</details>

<details>
<summary><strong>Linux（Wayland + NVIDIA）：网页内容点不动、缩放后黑屏</strong></summary>

AppImage 会强制 `GDK_BACKEND=x11`（走 XWayland）以规避历史上的原生 Wayland 崩溃。但在较新的 Wayland + NVIDIA 环境下，这会导致网页内容区点不动（标题栏按钮仍可点）、窗口缩放后黑屏。可用内置的逃生开关切回原生 Wayland：

```bash
CC_SWITCH_GDK_BACKEND=wayland ./CC-Switch-*.AppImage
```

如果你是从桌面图标启动的，请把它写进 `.desktop` 的 `Exec=` 行（如 `env CC_SWITCH_GDK_BACKEND=wayland /path/to/AppImage`），或在会话环境中设置。该变量是通用的：在 tiling Wayland 合成器（sway/Hyprland）下若出现点击失效，可反过来设 `CC_SWITCH_GDK_BACKEND=x11`。不设置则保持默认行为。

</details>

## 文档

如需了解各项功能的详细使用方法，请查阅 **[用户手册](docs/user-manual/zh/README.md)** — 涵盖供应商管理、MCP/Prompts/Skills、代理与故障转移等全部功能。

## 快速开始

### 基本使用

1. **添加供应商**：点击"添加供应商" → 选择预设或创建自定义配置
2. **切换供应商**：
   - 主界面：选择供应商 → 点击"启用"
   - 系统托盘：直接点击供应商名称（立即生效）
3. **生效方式**：重启终端或对应的 CLI 工具以应用更改（CLaude Code 无需重启）
4. **恢复官方登录**：添加"官方登录"预设，重启 CLI 工具后按照其登录/OAuth 流程操作

### MCP、Prompts、Skills 与会话

- **MCP**：点击"MCP"按钮 → 通过模板或自定义配置添加服务器 → 切换各应用同步开关
- **Prompts**：点击"Prompts" → 使用 Markdown 编辑器创建预设 → 激活后同步到 live 文件
- **Skills**：点击"Skills" → 浏览 GitHub 仓库 → 一键安装到支持的应用
- **会话**：点击"Sessions" → 浏览、搜索和恢复支持的会话来源

> **注意**：首次启动可以手动导入现有 CLI 工具配置作为默认供应商。

## 下载安装

### 系统要求

- **本 fork 仅发布 Windows x64 构建**（Windows 10+）。macOS / Linux 安装包**不由** `jiugjk/cc-switch` 发布 — 需要这些平台请使用 [上游 Releases](https://github.com/farion1231/cc-switch/releases)。

### Windows 用户（本 fork）

从 [Releases](../../releases) 页面下载最新版本。产物通常包括：

| 文件 | 说明 |
|------|------|
| `*.exe`（NSIS） | 推荐安装器 |
| `*.msi` | MSI 安装包 |
| `CC-Switch-portable-x64.exe` | 便携版（若存在） |

> **说明**
> - **未代码签名**：Windows SmartScreen 可能提示，选「仍要运行」即可。
> - **无自动更新通道**：本 fork 已移除应用内更新器（`createUpdaterArtifacts` 为 false、无 `plugins.updater` 配置、无签名密钥），「检查更新」会打开本 fork 的发布页手动下载。
> - 标签形如 `v3.19.1-fork.<run_number>`，并指向实际构建的提交。

<details>
<summary><strong>架构总览</strong></summary>

### 设计原则

```
┌─────────────────────────────────────────────────────────────┐
│                    前端 (React + TS)                         │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │ Components  │  │    Hooks     │  │  TanStack Query  │    │
│  │   （UI）     │──│ （业务逻辑）   │──│   （缓存/同步）    │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└────────────────────────┬────────────────────────────────────┘
                         │ Tauri IPC
┌────────────────────────▼────────────────────────────────────┐
│                  后端 (Tauri + Rust)                         │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │  Commands   │  │   Services   │  │  Models/Config   │    │
│  │ （API 层）   │──│  （业务层）    │──│    （数据）       │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**核心设计模式**

- **SSOT**（单一事实源）：所有数据存储在 `~/.cc-switch/cc-switch.db`（SQLite）
- **双层存储**：SQLite 存储可同步数据，JSON 存储设备级设置
- **双向同步**：切换时写入 live 文件，编辑当前供应商时从 live 回填
- **原子写入**：临时文件 + 重命名模式防止配置损坏
- **并发安全**：Mutex 保护的数据库连接避免竞态条件
- **分层架构**：清晰分离（Commands → Services → DAO → Database）

**核心组件**

- **ProviderService**：供应商增删改查、切换、回填、排序
- **McpService**：MCP 服务器管理、导入导出、live 文件同步
- **ProxyService**：本地 Proxy 模式，支持热切换和格式转换
- **SessionManager**：全应用会话历史浏览
- **ConfigService**：配置导入导出、备份轮换
- **SpeedtestService**：API 端点延迟测量

</details>

<details>
<summary><strong>开发指南</strong></summary>

### 环境要求

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### 开发命令

```bash
# 安装依赖
pnpm install

# 开发模式（热重载）
pnpm dev

# 类型检查
pnpm typecheck

# 代码格式化
pnpm format

# 检查代码格式
pnpm format:check

# 运行前端单元测试
pnpm test:unit

# 监听模式运行测试（推荐开发时使用）
pnpm test:unit:watch

# 构建应用
pnpm build

# 构建调试版本
pnpm tauri build --debug
```

### Rust 后端开发

```bash
cd src-tauri

# 格式化 Rust 代码
cargo fmt

# 运行 clippy 检查
cargo clippy

# 运行后端测试
cargo test

# 运行特定测试
cargo test test_name

# 运行带测试 hooks 的测试
cargo test --features test-hooks
```

### 测试说明

**前端测试**：

- 使用 **vitest** 作为测试框架
- 使用 **MSW (Mock Service Worker)** 模拟 Tauri API 调用
- 使用 **@testing-library/react** 进行组件测试

**运行测试**：

```bash
# 运行所有测试
pnpm test:unit

# 监听模式（自动重跑）
pnpm test:unit:watch

# 带覆盖率报告
pnpm test:unit --coverage
```

### 技术栈

**前端**：React 18 · TypeScript · Vite · TailwindCSS 3.4 · TanStack Query v5 · react-i18next · react-hook-form · zod · shadcn/ui · @dnd-kit

**后端**：Tauri 2.8 · Rust · serde · tokio · thiserror · tauri-plugin-process/dialog/store/log

**测试**：vitest · MSW · @testing-library/react

</details>

<details>
<summary><strong>项目结构</strong></summary>

```
├── src/                        # 前端 (React + TypeScript)
│   ├── components/
│   │   ├── providers/          # 供应商管理
│   │   ├── mcp/                # MCP 面板
│   │   ├── prompts/            # Prompts 管理
│   │   ├── skills/             # Skills 管理
│   │   ├── sessions/           # 会话管理器
│   │   ├── proxy/              # Proxy 模式面板
│   │   ├── openclaw/           # OpenClaw 配置面板
│   │   ├── settings/           # 设置（终端/备份/关于）
│   │   ├── deeplink/           # Deep Link 导入
│   │   ├── env/                # 环境变量管理
│   │   ├── universal/          # 跨应用配置
│   │   ├── usage/              # 用量统计
│   │   └── ui/                 # shadcn/ui 组件库
│   ├── hooks/                  # 自定义 hooks（业务逻辑）
│   ├── lib/
│   │   ├── api/                # Tauri API 封装（类型安全）
│   │   └── query/              # TanStack Query 配置
│   ├── locales/                # 翻译 (zh/zh-TW/en/ja)
│   ├── config/                 # 预设 (providers/mcp)
│   └── types/                  # TypeScript 类型定义
├── src-tauri/                  # 后端 (Rust)
│   └── src/
│       ├── commands/           # Tauri 命令层（按领域）
│       ├── services/           # 业务逻辑层
│       ├── database/           # SQLite DAO 层
│       ├── proxy/              # Proxy 模块
│       ├── session_manager/    # 会话管理
│       ├── deeplink/           # Deep Link 处理
│       └── mcp/                # MCP 同步模块
├── tests/                      # 前端测试
└── assets/                     # 截图 & 合作商资源
```

</details>

## 贡献

欢迎提交 Issue 反馈问题和建议！

提交 PR 前请确保：

- 通过类型检查：`pnpm typecheck`
- 通过格式检查：`pnpm format:check`
- 通过单元测试：`pnpm test:unit`

新功能开发前，欢迎先开 Issue 讨论实现方案，不适合项目的功能性 PR 有可能会被关闭。

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=jiugjk/cc-switch&type=Date)](https://www.star-history.com/#jiugjk/cc-switch&Date)

## 致谢

本项目 fork 自 [farion1231/cc-switch](https://github.com/farion1231/cc-switch)（作者 Jason Young）。主体功能仍属上游工作，感谢原作者与贡献者。

赞助商列表由上游维护（本 fork 不再镜像）：[上游赞助商](https://github.com/farion1231/cc-switch/blob/main/README_ZH.md).

## License

MIT © Jason Young

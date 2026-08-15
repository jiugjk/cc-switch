<div align="center">

# CC Switch

### The All-in-One Manager for Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build, OpenCode, OpenClaw, Hermes & Pi

[![Version](https://img.shields.io/github/v/release/jiugjk/cc-switch?color=blue&label=version)](https://github.com/jiugjk/cc-switch/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20x64-lightgrey.svg)](https://github.com/jiugjk/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![Downloads](https://img.shields.io/github/downloads/jiugjk/cc-switch/total)](https://github.com/jiugjk/cc-switch/releases/latest)

<a href="https://trendshift.io/repositories/15372" target="_blank"><img src="https://trendshift.io/api/badge/repositories/15372" alt="farion1231%2Fcc-switch | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>
<a href="https://www.star-history.com/#jiugjk/cc-switch&Date"><picture><source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/badge?repo=farion1231/cc-switch&theme=dark" /><img alt="Star History Rank" src="https://api.star-history.com/badge?repo=farion1231/cc-switch" width="196" height="55" /></picture></a>

### 🌐 Upstream project website: **[ccswitch.io](https://ccswitch.io)**

[中文](README.md) | English | [Changelog](CHANGELOG.md)

</div>

## About This Distribution

This is an independently maintained Windows distribution derived from the MIT-licensed [farion1231/cc-switch](https://github.com/farion1231/cc-switch), with its own release cadence. The current app version is **v3.19.2**, including later upstream merges (notably **Pi** as a native coding agent). On top of upstream, this distribution adds:

### CodexCont & proxy

- **CodexCont reasoning auto-continuation** — Toggle under `Settings → Routing → CodexCont` continues truncated reasoning on native `/v1/responses` chains. Engages only when it will not trigger Responses→Chat/Anthropic conversion; fully reuses `RequestForwarder::forward_with_retry` (no bypass, no pinned provider). Does not swallow rounds that carry tool calls; keeps legacy streamed `function_call` events.
- **Route observability** — `ProxyStatus` exposes last-route fields; General settings shows `ProxyStatusSummary` (active provider, last success/fail, fallback reason).
- **Quota attribution** — Classifies quota-exhaustion errors and records `last_fallback_reason`; optional `decouple_official_quota` keeps official auth from forcing custom-API failover. No second independent “quota-fallback” switch — refinements go through the existing failover path.
- **Provider capability resolver** — Declared/heuristic capability snapshot (including Chat = degraded continuation) for safer routing decisions.
- **Guides** — [CodexCont behavior, cost and safety gates](docs/guides/codex-continuation-guide-en.md) ([中文](docs/guides/codex-continuation-guide-zh.md) · [日本語](docs/guides/codex-continuation-guide-ja.md)).

### Apps, detection & UI

- **Top app switcher** — Auto-hides uninstalled tools; re-probes when the window regains focus; overflows into a “more” popover so action buttons stay on-screen.
- **Codex desktop** — Detects the Microsoft Store `OpenAI.Codex` package on Windows; endpoint hints follow the selected upstream API format.
- **Localized environment-check text** — `not installed or not executable` follows the active UI language.
- **Tool website links** — Shortcut next to each tool name opens its official site.
- **Updated Windows install commands** — Per-tool one-click install commands refreshed.
- **Grok Build configuration ownership** — Provider switching changes only model/endpoint profiles while preserving global TOML, MCP and future settings; supports `GROK_CONFIG` / `GROK_HOME`, full-file editing, privacy drafts and local restoreable backups. See the [configuration guide](docs/guides/grok-build-config-guide-en.md) ([中文](docs/guides/grok-build-config-guide-zh.md) · [日本語](docs/guides/grok-build-config-guide-ja.md)).

### Build

- **Free Windows auto-build** — GitHub Actions builds **unsigned** NSIS / MSI / portable artifacts on free hosted runners after CI is green on `main` (or via `workflow_dispatch` on `main`). This distribution has no macOS/Linux installers and no code signing / auto-update channel.

Open user-side checks: real e2e matrix with proxy on/off; live confirm that ChatGPT desktop traffic hits `127.0.0.1:15721` `/v1/responses` when takeover is enabled.

## Why CC Switch?

Modern AI-powered coding relies on tools like Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build, OpenCode, OpenClaw, Hermes, and Pi — but each has its own configuration format. Switching API providers means manually editing JSON, TOML, or `.env` files, and there is no unified way to manage MCP and Skills across multiple tools.

**CC Switch** gives you a single desktop app to manage all supported AI tools. Instead of editing config files by hand, you get a visual interface to import providers with one click, switch between them instantly, with 50+ built-in provider presets, unified MCP and Skills management, and system tray quick switching — all backed by a reliable SQLite database with atomic writes that protect your configs from corruption.

- **One App, Nine Tools** — Manage Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build, OpenCode, OpenClaw, Hermes, and Pi from a single interface
- **No More Manual Editing** — 50+ provider presets including AWS Bedrock, NVIDIA NIM, and community relays; just pick and switch
- **Unified MCP & Skills Management** — One panel to manage MCP servers and Skills across Claude, Codex, Gemini, Grok Build, OpenCode, Hermes (and Pi on the Skills side) with bidirectional sync
- **System Tray Quick Switch** — Switch providers instantly from the tray menu, no need to open the full app
- **Cloud Sync** — Sync data across devices via Dropbox, OneDrive, iCloud, WebDAV, or S3-compatible storage
- **Cross-Platform (upstream)** — Upstream supports Windows, macOS, and Linux; **this distribution's published installers are Windows x64 only**
- **Built-in Utilities** — Includes various utilities for first-launch login confirmation, signature bypass, plugin extension sync, and more

## Screenshots

|                  Main Interface                   |                  Add Provider                  |
| :-----------------------------------------------: | :--------------------------------------------: |
| ![Main Interface](assets/screenshots/main-en.png) | ![Add Provider](assets/screenshots/add-en.png) |

## Features

[Full Changelog](CHANGELOG.md) | [Release Notes](docs/release-notes/v3.19.2-en.md)

### Provider Management

- **9 supported tools, 50+ presets** — Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build, OpenCode, OpenClaw, Hermes, Pi; copy your key and import with one click
- **Switch vs additive mode** — Claude / Claude Desktop / Codex / Gemini / Grok Build write only the current provider; OpenCode / OpenClaw / Hermes / Pi write every provider into the live config
- **Universal providers** — One config syncs to Claude Code, Codex, and Gemini CLI
- **Pi native providers** — Manages explicit provider nodes in `models.json` only; does not take over Pi `/login`, `auth.json`, or the default model. See the [Pi native contract](docs/pi-native-contract-zh.md)
- One-click switching, system tray quick access, drag-and-drop sorting, import/export

### Proxy & Failover

- **Local proxy with hot-switching** — Format conversion, auto-failover, circuit breaker, provider health monitoring, and request rectifier
- **App-level takeover** — Independently proxy Claude, Codex, Gemini, or Grok Build, down to individual providers

### MCP, Prompts & Skills

- **Unified MCP panel** — Manage MCP servers across Claude, Codex, Gemini, Grok Build, OpenCode, and Hermes (Pi / OpenClaw / Claude Desktop have no native MCP registry) with bidirectional sync and Deep Link import
- **Search and bulk toggles** — Search MCP / Prompts / Skills; bulk-enable or disable an app across the MCP and Skills lists
- **Prompts** — Markdown editor with cross-app sync (CLAUDE.md / AGENTS.md / GEMINI.md / Hermes `SOUL.md`); Pi also has native `SYSTEM.md` and slash-command templates
- **Skills** — One-click install from GitHub repos or ZIP files, custom repository management, with symlink and file copy support (including Pi)

### Usage & Cost Tracking

- **Usage dashboard** — Track spending, requests, and tokens with trend charts, detailed request logs, and custom per-model pricing
- **models.dev auto pricing** — Optional price sync from models.dev; local overrides live in `~/.cc-switch/model-pricing.json`
- **Auth Center** — Per-account ChatGPT (Codex OAuth) subscription usage; official Grok / SuperGrok quota on provider cards

### Session Manager & Workspace

- Browse, search, and restore conversation history across supported session sources (including Pi)
- **Workspace editor** (OpenClaw) — Edit agent files (AGENTS.md, SOUL.md, etc.) with Markdown preview

### System & Platform

- **Cloud sync** — Custom config directory (Dropbox, OneDrive, iCloud, NAS), WebDAV, and S3-compatible storage
- **Deep Link** (`ccswitch://`) — Import providers, MCP servers, prompts, and skills via URL
- Dark / Light / System theme, auto-launch, atomic writes, auto-backups, i18n (zh/zh-TW/en/ja)
- **Default UI language is Simplified Chinese**; follow the OS or change it in Settings

## FAQ

<details>
<summary><strong>Which AI tools does CC Switch support?</strong></summary>

CC Switch supports nine tools: **Claude Code**, **Claude Desktop**, **Codex**, **Gemini CLI**, **Grok Build**, **OpenCode**, **OpenClaw**, **Hermes**, and **Pi**. Each tool has dedicated provider presets and configuration management.

</details>

<details>
<summary><strong>Do I need to restart the terminal after switching providers?</strong></summary>

For most tools, yes — restart your terminal or the CLI tool for changes to take effect. The exception is **Claude Code**, which currently supports hot-switching of provider data without a restart.

</details>

<details>
<summary><strong>My plugin configuration disappeared after switching providers — what happened?</strong></summary>

CC Switch provides a "Shared Config Snippet" feature to pass common data (beyond API keys and endpoints) between providers. Go to "Edit Provider" → "Shared Config Panel" → click "Extract from Current Provider" to save all common data. When creating a new provider, check "Apply Shared Config" (enabled by default) to include plugin data in the new provider. All your configuration items are preserved in the default provider imported when you first launched the app.

</details>

<details>
<summary><strong>Windows SmartScreen warning</strong></summary>

This distribution ships **unsigned** installers. Windows SmartScreen may warn; choose "Run anyway". For macOS / Linux packages, use [upstream releases](https://github.com/farion1231/cc-switch/releases).

</details>

<details>
<summary><strong>Why can't I delete the currently active provider?</strong></summary>

CC Switch follows a "minimal intrusion" design principle — even if you uninstall the app, your CLI tools will continue to work normally. The system always keeps one active configuration, because deleting all configurations would make the corresponding CLI tool unusable. If you rarely use a specific CLI tool, you can hide it in Settings. To switch back to official login, see the next question.

</details>

<details>
<summary><strong>How do I switch back to official login?</strong></summary>

Add an official provider from the preset list. After switching to it, run the Log out / Log in flow, and then you can freely switch between the official provider and third-party providers. Codex supports switching between different official providers, making it easy to switch between multiple Plus or Team accounts. Pi login stays with Pi's own `/login`; CC Switch does not read or write `auth.json`.

</details>

<details>
<summary><strong>Where is my data stored?</strong></summary>

- **Database**: `~/.cc-switch/cc-switch.db` (SQLite — providers, MCP, prompts, skills)
- **Local settings**: `~/.cc-switch/settings.json` (device-level UI preferences)
- **Backups**: `~/.cc-switch/backups/` (auto-rotated, keeps 10 most recent)
- **Skills**: `~/.cc-switch/skills/` (symlinked to corresponding apps by default)
- **Skill Backups**: `~/.cc-switch/skill-backups/` (created automatically before uninstall, keeps 20 most recent)
- **Model pricing overrides**: `~/.cc-switch/model-pricing.json` (optional; models.dev sync and manual edits)

On Windows, `~` is your user profile, e.g. `C:\Users\<you>\.cc-switch\`.

</details>

## Documentation

For detailed guides on every feature, check out the **[User Manual](docs/user-manual/en/README.md)** — covering provider management, MCP/Prompts/Skills, proxy & failover, and more.

Pi integration boundaries: [Pi native contract](docs/pi-native-contract-zh.md) (Chinese). Grok Build ownership: [configuration guide](docs/guides/grok-build-config-guide-en.md).

## Quick Start

### Basic Usage

1. **Add Provider**: Click "Add Provider" → Choose a preset or create custom configuration
2. **Switch Provider**:
   - Main UI: Select provider → Click "Enable"
   - System Tray: Click provider name directly (instant effect)
3. **Takes Effect**: Restart your terminal or the corresponding CLI tool to apply changes (Claude Code does not require a restart)
4. **Back to Official**: Add an "Official Login" preset, restart the CLI tool, then follow its login/OAuth flow

### MCP, Prompts, Skills & Sessions

- **MCP**: Click the "MCP" button → Add servers via templates or custom config → Toggle per-app sync
- **Prompts**: Click "Prompts" → Create presets with Markdown editor → Activate to sync to live files
- **Skills**: Click "Skills" → Browse GitHub repos → One-click install to supported apps
- **Sessions**: Click "Sessions" → Browse, search, and restore conversation history across supported session sources

> **Note**: On first launch, you can manually import existing CLI tool configs as the default provider.

## Download & Installation

### System Requirements

- **This distribution ships Windows x64 builds only** (Windows 10+). macOS / Linux packages are **not** published by `jiugjk/cc-switch` — use [upstream releases](https://github.com/farion1231/cc-switch/releases) if you need those platforms.

### Windows Users

Download the latest release from the [Releases](../../releases) page. Assets typically include:

| Asset | Notes |
|-------|--------|
| `*.exe` (NSIS) | Recommended installer |
| `*.msi` | MSI installer |
| `CC-Switch-portable-x64.exe` | Portable (when present) |

> **Notes**
> - Builds are **unsigned** — Windows SmartScreen may warn; choose "Run anyway".
> - No auto-update channel — the in-app updater is removed (`createUpdaterArtifacts` is false, no `plugins.updater` config, no signing key). "Check for Updates" opens this distribution's releases page for manual download.
> - Automated build tags look like `v3.19.2-windows.<run_number>` and point at the commit that was built.

<details>
<summary><strong>Architecture Overview</strong></summary>

### Design Principles

```
┌─────────────────────────────────────────────────────────────┐
│                    Frontend (React + TS)                    │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │ Components  │  │    Hooks     │  │  TanStack Query  │    │
│  │   (UI)      │──│ (Bus. Logic) │──│   (Cache/Sync)   │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└────────────────────────┬────────────────────────────────────┘
                         │ Tauri IPC
┌────────────────────────▼────────────────────────────────────┐
│                  Backend (Tauri + Rust)                     │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │  Commands   │  │   Services   │  │  Models/Config   │    │
│  │ (API Layer) │──│ (Bus. Layer) │──│     (Data)       │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Core Design Patterns**

- **SSOT** (Single Source of Truth): All data stored in `~/.cc-switch/cc-switch.db` (SQLite)
- **Dual-layer Storage**: SQLite for syncable data, JSON for device-level settings
- **Dual-way Sync**: Write to live files on switch, backfill from live when editing active provider
- **Atomic Writes**: Temp file + rename pattern prevents config corruption (`ReplaceFileW` on Windows)
- **Concurrency Safe**: Mutex-protected database connection avoids race conditions
- **Layered Architecture**: Clear separation (Commands → Services → DAO → Database)

**Key Components**

- **ProviderService**: Provider CRUD, switching, backfill, sorting
- **McpService**: MCP server management, import/export, live file sync
- **ProxyService**: Local proxy mode with hot-switching and format conversion
- **SessionManager**: Conversation history browsing across supported session sources
- **ConfigService**: Config import/export, backup rotation
- **SpeedtestService**: API endpoint latency measurement

</details>

<details>
<summary><strong>Development Guide</strong></summary>

### Environment Requirements

- Node.js 18+
- pnpm 10+ (pinned via `packageManager` in `package.json`; `corepack enable` recommended)
- Rust 1.88+ (this repo pins 1.95 in `rust-toolchain.toml`)
- Tauri CLI 2.8+

### Development Commands

```bash
# Install dependencies
pnpm install

# Dev mode (hot reload)
pnpm dev

# Type check
pnpm typecheck

# Format code
pnpm format

# Check code format
pnpm format:check

# Run frontend unit tests
pnpm test:unit

# Run tests in watch mode (recommended for development)
pnpm test:unit:watch

# Build application
pnpm build

# Build debug version
pnpm tauri build --debug
```

### Rust Backend Development

```bash
cd src-tauri

# Format Rust code
cargo fmt

# Run clippy checks
cargo clippy

# Run backend tests
cargo test

# Run specific tests
cargo test test_name

# Run tests with test-hooks feature
cargo test --features test-hooks
```

### Testing Guide

**Frontend Testing**:

- Uses **vitest** as test framework
- Uses **MSW (Mock Service Worker)** to mock Tauri API calls
- Uses **@testing-library/react** for component testing

**Running Tests**:

```bash
# Run all tests
pnpm test:unit

# Watch mode (auto re-run)
pnpm test:unit:watch

# With coverage report
pnpm test:unit --coverage
```

### Tech Stack

**Frontend**: React 18 · TypeScript · Vite · TailwindCSS 3.4 · TanStack Query v5 · react-i18next · react-hook-form · zod · shadcn/ui · @dnd-kit

**Backend**: Tauri 2.8 · Rust · serde · tokio · thiserror · tauri-plugin-process/dialog/store/log/deep-link

**Testing**: vitest · MSW · @testing-library/react

</details>

<details>
<summary><strong>Project Structure</strong></summary>

```
├── src/                        # Frontend (React + TypeScript)
│   ├── components/
│   │   ├── providers/          # Provider management
│   │   ├── mcp/                # MCP panel
│   │   ├── prompts/            # Prompts (including Pi native)
│   │   ├── skills/             # Skills management
│   │   ├── sessions/           # Session Manager
│   │   ├── proxy/              # Proxy mode panel
│   │   ├── openclaw/           # OpenClaw config panels
│   │   ├── workspace/          # OpenClaw workspace
│   │   ├── profiles/           # Project / workspace switcher
│   │   ├── settings/           # Settings (Terminal/Backup/About)
│   │   ├── deeplink/           # Deep Link import
│   │   ├── env/                # Environment variable management
│   │   ├── universal/          # Cross-app configuration
│   │   ├── usage/              # Usage statistics
│   │   └── ui/                 # shadcn/ui component library
│   ├── hooks/                  # Custom hooks (business logic)
│   ├── lib/
│   │   ├── api/                # Tauri API wrapper (type-safe)
│   │   └── query/              # TanStack Query config
│   ├── i18n/locales/           # Translations (zh/zh-TW/en/ja)
│   ├── config/                 # Presets (providers/mcp)
│   └── types/                  # TypeScript definitions
├── src-tauri/                  # Backend (Rust)
│   └── src/
│       ├── commands/           # Tauri command layer (by domain)
│       ├── services/           # Business logic layer
│       ├── database/           # SQLite DAO layer
│       ├── proxy/              # Proxy module
│       ├── session_manager/    # Session management
│       ├── deeplink/           # Deep Link handling
│       ├── mcp/                # MCP sync module
│       └── pi_config/          # Pi models.json adapter
├── tests/                      # Frontend tests
└── assets/                     # Screenshots & partner resources
```

</details>

## Contributing

Issues and suggestions are welcome!

Before submitting PRs, please ensure:

- Pass type check: `pnpm typecheck`
- Pass format check: `pnpm format:check`
- Pass unit tests: `pnpm test:unit`

For new features, please open an issue for discussion before submitting a PR. PRs for features that are not a good fit for the project may be closed.

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=jiugjk/cc-switch&type=Date)](https://www.star-history.com/#jiugjk/cc-switch&Date)

## Acknowledgments

This distribution is derived from the MIT-licensed [farion1231/cc-switch](https://github.com/farion1231/cc-switch) by Jason Young. Most of the product remains upstream work — thank you to the original author and contributors. Independent maintenance and release metadata do not remove the original copyright or license notices.

The Grok Build global-configuration workflow was informed by the MIT-licensed [2836048681/cc-switch-codexcont](https://github.com/2836048681/cc-switch-codexcont); this implementation was adapted to this distribution's Responses-only proxy route, database-owned MCP projection, and existing transaction backup model.

Sponsor listings are maintained upstream (not mirrored here): [upstream sponsors](https://github.com/farion1231/cc-switch#heartsponsor).

## License

MIT © Jason Young

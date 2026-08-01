use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::app_config::{AppType, McpApps, McpServer};
use crate::error::AppError;
use crate::mcp;
use crate::store::AppState;

/// MCP 相关业务逻辑（v3.7.0 统一结构）
pub struct McpService;

static MCP_LIVE_TRANSACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
type McpTransitionTestHook = std::sync::Arc<dyn Fn(&str) + Send + Sync>;
#[cfg(test)]
static MCP_TRANSITION_TEST_HOOK: OnceLock<Mutex<Option<McpTransitionTestHook>>> = OnceLock::new();

struct LiveFileSnapshot {
    app: AppType,
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

impl LiveFileSnapshot {
    fn capture(app: AppType, path: PathBuf) -> Result<Self, AppError> {
        let contents = match std::fs::read(&path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(AppError::io(&path, error)),
        };
        Ok(Self {
            app,
            path,
            contents,
        })
    }

    fn restore(&self) -> Result<(), AppError> {
        match &self.contents {
            Some(contents) => crate::config::atomic_write(&self.path, contents),
            None => match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(AppError::io(&self.path, error)),
            },
        }
    }
}

impl McpService {
    /// 获取所有 MCP 服务器（统一结构）
    pub fn get_all_servers(state: &AppState) -> Result<IndexMap<String, McpServer>, AppError> {
        state.db.get_all_mcp_servers()
    }

    /// 添加或更新 MCP 服务器
    pub fn upsert_server(state: &AppState, server: McpServer) -> Result<(), AppError> {
        let _transaction_guard = Self::lock_live_transaction()?;
        Self::upsert_server_locked(state, server)
    }

    fn upsert_server_locked(state: &AppState, server: McpServer) -> Result<(), AppError> {
        let previous = state.db.get_all_mcp_servers()?.get(&server.id).cloned();
        let live_snapshots = Self::capture_live_snapshots(previous.as_ref(), Some(&server))?;

        #[cfg(test)]
        Self::run_transition_test_hook(&server.id);

        if let Err(error) = Self::apply_live_transition(state, previous.as_ref(), &server) {
            let rollback_failures = Self::restore_live_snapshots(&live_snapshots);
            return Err(Self::transition_error(
                "更新 MCP live 配置失败",
                error,
                rollback_failures,
            ));
        }

        if let Err(error) = state.db.save_mcp_server(&server) {
            let rollback_failures = Self::restore_live_snapshots(&live_snapshots);
            return Err(Self::transition_error(
                "保存 MCP 数据库状态失败",
                error,
                rollback_failures,
            ));
        }

        Ok(())
    }

    /// 删除 MCP 服务器
    pub fn delete_server(state: &AppState, id: &str) -> Result<bool, AppError> {
        let _transaction_guard = Self::lock_live_transaction()?;
        let server = state.db.get_all_mcp_servers()?.shift_remove(id);

        let Some(server) = server else {
            return Ok(false);
        };
        let live_snapshots = Self::capture_live_snapshots(Some(&server), None)?;

        if let Err(error) = Self::remove_server_from_all_apps(state, id, &server) {
            let rollback_failures = Self::restore_live_snapshots(&live_snapshots);
            return Err(Self::transition_error(
                "删除 MCP live 配置失败",
                error,
                rollback_failures,
            ));
        }

        if let Err(error) = state.db.delete_mcp_server(id) {
            let rollback_failures = Self::restore_live_snapshots(&live_snapshots);
            return Err(Self::transition_error(
                "删除 MCP 数据库状态失败",
                error,
                rollback_failures,
            ));
        }

        Ok(true)
    }

    /// 切换指定应用的启用状态
    pub fn toggle_app(
        state: &AppState,
        server_id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        let _transaction_guard = Self::lock_live_transaction()?;
        let Some(mut server) = state.db.get_all_mcp_servers()?.get(server_id).cloned() else {
            return Ok(());
        };
        server.apps.set_enabled_for(&app, enabled);
        Self::upsert_server_locked(state, server)?;

        Ok(())
    }

    fn apply_live_transition(
        state: &AppState,
        previous: Option<&McpServer>,
        next: &McpServer,
    ) -> Result<(), AppError> {
        for app in AppType::all() {
            let was_enabled = previous.is_some_and(|server| server.apps.is_enabled_for(&app));
            let will_be_enabled = next.apps.is_enabled_for(&app);
            if was_enabled && !will_be_enabled {
                Self::remove_server_from_app(state, &next.id, &app)?;
            }
        }

        // Re-project every enabled target, including unchanged flags, because the server
        // command/url/env itself may have changed.
        Self::sync_server_to_apps(state, next)
    }

    fn lock_live_transaction() -> Result<MutexGuard<'static, ()>, AppError> {
        MCP_LIVE_TRANSACTION_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|error| AppError::Message(format!("MCP live 事务锁已损坏: {error}")))
    }

    #[cfg(test)]
    fn run_transition_test_hook(server_id: &str) {
        let hook = MCP_TRANSITION_TEST_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook(server_id);
        }
    }

    fn live_config_path(app: &AppType) -> Option<PathBuf> {
        match app {
            AppType::Claude => Some(crate::config::get_claude_mcp_path()),
            AppType::Codex => Some(crate::codex_config::get_codex_config_path()),
            AppType::Gemini => Some(crate::gemini_config::get_gemini_settings_path()),
            AppType::GrokBuild => Some(crate::grok_config::get_grok_config_path()),
            AppType::OpenCode => Some(crate::opencode_config::get_opencode_config_path()),
            AppType::Hermes => Some(crate::hermes_config::get_hermes_config_path()),
            AppType::ClaudeDesktop | AppType::OpenClaw => None,
        }
    }

    fn capture_live_snapshots(
        previous: Option<&McpServer>,
        attempted: Option<&McpServer>,
    ) -> Result<Vec<LiveFileSnapshot>, AppError> {
        let mut snapshots = Vec::new();
        for app in AppType::all() {
            let was_enabled = previous.is_some_and(|server| server.apps.is_enabled_for(&app));
            let was_touched = attempted.is_some_and(|server| server.apps.is_enabled_for(&app));
            if !was_enabled && !was_touched {
                continue;
            }
            if let Some(path) = Self::live_config_path(&app) {
                snapshots.push(LiveFileSnapshot::capture(app, path)?);
            }
        }
        Ok(snapshots)
    }

    fn restore_live_snapshots(snapshots: &[LiveFileSnapshot]) -> Vec<String> {
        let mut failures = Vec::new();
        for snapshot in snapshots.iter().rev() {
            if let Err(error) = snapshot.restore() {
                failures.push(format!("{}: {error}", snapshot.app.as_str()));
            }
        }
        failures
    }

    fn transition_error(
        context: &str,
        error: AppError,
        rollback_failures: Vec<String>,
    ) -> AppError {
        if rollback_failures.is_empty() {
            error
        } else {
            AppError::Message(format!(
                "{context}: {error}; live 回滚也失败: {}",
                rollback_failures.join("; ")
            ))
        }
    }

    /// 将 MCP 服务器同步到所有启用的应用
    fn sync_server_to_apps(_state: &AppState, server: &McpServer) -> Result<(), AppError> {
        for app in server.apps.enabled_apps() {
            Self::sync_server_to_app_no_config(server, &app)?;
        }

        Ok(())
    }

    /// 将 MCP 服务器同步到指定应用
    fn sync_server_to_app(
        _state: &AppState,
        server: &McpServer,
        app: &AppType,
    ) -> Result<(), AppError> {
        Self::sync_server_to_app_no_config(server, app)
    }

    fn sync_server_to_app_no_config(server: &McpServer, app: &AppType) -> Result<(), AppError> {
        match app {
            AppType::Claude => {
                mcp::sync_single_server_to_claude(&Default::default(), &server.id, &server.server)?;
            }
            AppType::ClaudeDesktop => {
                log::debug!("Claude Desktop 3P profiles do not use CC Switch MCP sync, skipping");
            }
            AppType::Codex => {
                // Codex uses TOML format, must use the correct function
                mcp::sync_single_server_to_codex(&Default::default(), &server.id, &server.server)?;
            }
            AppType::Gemini => {
                mcp::sync_single_server_to_gemini(&Default::default(), &server.id, &server.server)?;
            }
            AppType::GrokBuild => {
                mcp::sync_single_server_to_grokbuild(
                    &Default::default(),
                    &server.id,
                    &server.server,
                )?;
            }
            AppType::OpenCode => {
                mcp::sync_single_server_to_opencode(
                    &Default::default(),
                    &server.id,
                    &server.server,
                )?;
            }
            AppType::OpenClaw => {
                // OpenClaw MCP support is still in development (Issue #4834)
                // Skip for now
                log::debug!("OpenClaw MCP support is still in development, skipping sync");
            }
            AppType::Hermes => {
                mcp::sync_single_server_to_hermes(&Default::default(), &server.id, &server.server)?;
            }
        }
        Ok(())
    }

    /// 从所有曾启用过该服务器的应用中移除
    fn remove_server_from_all_apps(
        state: &AppState,
        id: &str,
        server: &McpServer,
    ) -> Result<(), AppError> {
        // 从所有曾启用的应用中移除
        for app in server.apps.enabled_apps() {
            Self::remove_server_from_app(state, id, &app)?;
        }
        Ok(())
    }

    fn remove_server_from_app(_state: &AppState, id: &str, app: &AppType) -> Result<(), AppError> {
        match app {
            AppType::Claude => mcp::remove_server_from_claude(id)?,
            AppType::ClaudeDesktop => {
                log::debug!("Claude Desktop 3P profiles do not use CC Switch MCP sync, skipping");
            }
            AppType::Codex => mcp::remove_server_from_codex(id)?,
            AppType::Gemini => mcp::remove_server_from_gemini(id)?,
            AppType::GrokBuild => mcp::remove_server_from_grokbuild(id)?,
            AppType::OpenCode => {
                mcp::remove_server_from_opencode(id)?;
            }
            AppType::OpenClaw => {
                // OpenClaw MCP support is still in development
                log::debug!("OpenClaw MCP support is still in development, skipping remove");
            }
            AppType::Hermes => {
                mcp::remove_server_from_hermes(id)?;
            }
        }
        Ok(())
    }

    /// 手动同步所有启用的 MCP 服务器到对应的应用。
    ///
    /// Best-effort：单个应用投影失败（如 ~/.claude.json 坏 JSON）不阻断
    /// 其余应用——各应用的 live 文件互相独立，一处损坏没有理由让其他
    /// 应用的 MCP 状态陈旧。全部跑完后若有失败，聚合成一个错误上报，
    /// 保留调用方的可见性。
    pub fn sync_all_enabled(state: &AppState) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;

        let mut failures: Vec<String> = Vec::new();
        for app in AppType::all() {
            if let Err(err) = Self::project_servers_to_app(state, &servers, &app) {
                log::warn!("同步 MCP 到 {app:?} 失败: {err}");
                failures.push(format!("{}: {err}", app.as_str()));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "部分应用 MCP 同步失败: {}",
                failures.join("; ")
            )))
        }
    }

    /// 只把启用状态投影到单个应用。某个应用的 live 被整体重写后用它做
    /// 定向重投影，避免把无关应用的失败面（如 ~/.claude.json 坏 JSON）
    /// 牵连进目标应用的关键路径。
    pub fn sync_enabled_for_app(state: &AppState, app: &AppType) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;
        Self::project_servers_to_app(state, &servers, app)
    }

    fn project_servers_to_app(
        state: &AppState,
        servers: &IndexMap<String, McpServer>,
        app: &AppType,
    ) -> Result<(), AppError> {
        if matches!(app, AppType::OpenClaw | AppType::ClaudeDesktop) {
            return Ok(());
        }

        for server in servers.values() {
            if server.apps.is_enabled_for(app) {
                Self::sync_server_to_app(state, server, app)?;
            } else {
                Self::remove_server_from_app(state, &server.id, app)?;
            }
        }

        Ok(())
    }

    /// 整体重写 Codex / Grok Build 的 live config.toml 之前，把用户直接在
    /// live `[mcp_servers.*]` 里手工编辑的条目回填进 DB（MCP SSOT）。
    ///
    /// 背景：这两个应用的 MCP 与 live 同文件，且 live 写入是整文件替换 +
    /// 从 DB 重投影（见 provider 切换/保存路径）。若不先回填，用户在 live
    /// 里改过的 command/cwd 等会被 DB 里的陈旧规范写回（如 node_repl 换了
    /// 安装目录后每次切换都被打回旧路径）。
    ///
    /// 规则：
    /// - 仅 Codex / GrokBuild；其余应用直接返回 0（它们的 MCP 文件不随
    ///   live 重写被清空）。
    /// - live 与 DB 都有且该应用已启用：经有损投影归一化后比较（中和
    ///   headers/http_headers 命名、双向转换丢弃的复杂字段等 round-trip
    ///   差异），不同则以 live 为准更新规范；apps 标志、名称、标签等
    ///   元数据保持不变。
    /// - live 独有的 id：按导入语义新建仅启用该应用的行，避免用户手工
    ///   添加的服务器被整文件重写静默清掉。
    /// - live 缺失的 id：不动。缺失有歧义（重写与重投影之间崩溃时
    ///   [mcp_servers] 合法地为空），绝不据此禁用或删除。
    /// - 单条失败（校验/保存）跳过并告警，不阻断其余条目；调用方须
    ///   warn-degrade，绝不让回填失败中断切换/保存。
    ///
    /// 返回发生变更（更新或新建）的行数。
    pub fn backfill_live_edits_for_app(state: &AppState, app: &AppType) -> Result<usize, AppError> {
        type SpecsEquivalent = fn(&serde_json::Value, &serde_json::Value) -> bool;
        let (live_specs, specs_equivalent): (HashMap<String, serde_json::Value>, SpecsEquivalent) =
            match app {
                AppType::Codex => (
                    mcp::collect_live_codex_server_specs()?,
                    mcp::codex_specs_equivalent,
                ),
                AppType::GrokBuild => (
                    mcp::collect_live_grokbuild_server_specs()?,
                    mcp::grokbuild_specs_equivalent,
                ),
                _ => return Ok(0),
            };
        if live_specs.is_empty() {
            return Ok(0);
        }

        let mut servers = state.db.get_all_mcp_servers()?;
        let mut changed = 0usize;
        for (id, live_spec) in live_specs {
            if let Some(existing) = servers.get_mut(&id) {
                if !existing.apps.is_enabled_for(app) {
                    // 未对该应用启用的行不吸收 live 内容：重投影本就会把该
                    // 条目从 live 移除，回填反而会让它复活。
                    continue;
                }
                if specs_equivalent(&existing.server, &live_spec) {
                    continue;
                }
                existing.server = match app {
                    AppType::Codex => mcp::merge_codex_live_spec(&existing.server, &live_spec),
                    AppType::GrokBuild => {
                        mcp::merge_grokbuild_live_spec(&existing.server, &live_spec)
                    }
                    _ => live_spec,
                };
                if let Err(err) = state.db.save_mcp_server(existing) {
                    log::warn!("回填 live MCP 编辑到 '{id}' 失败: {err}");
                    continue;
                }
                log::info!("已把 live 中编辑过的 MCP 服务器 '{id}' 回填进 DB（live 优先）");
                changed += 1;
            } else {
                // live 独有：视作用户手工添加，按导入语义入库（仅启用当前应用）。
                // 注意：若在 CC Switch 内删除服务器时 live 移除曾静默失败，这里
                // 会把它重新导入——留日志便于排查。
                let mut apps = McpApps::default();
                apps.set_enabled_for(app, true);
                let server = McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: live_spec,
                    apps,
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                };
                if let Err(err) = state.db.save_mcp_server(&server) {
                    log::warn!("回填 live 新增 MCP 服务器 '{id}' 入库失败: {err}");
                    continue;
                }
                log::info!("live 中发现未知 MCP 服务器 '{id}'，已按导入语义入库（仅启用 {app:?}）");
                servers.insert(id, server);
                changed += 1;
            }
        }
        Ok(changed)
    }

    // ========================================================================
    // 兼容层：支持旧的 v3.6.x 命令（已废弃，将在 v4.0 移除）
    // ========================================================================

    /// [已废弃] 获取指定应用的 MCP 服务器（兼容旧 API）
    #[deprecated(since = "3.7.0", note = "Use get_all_servers instead")]
    pub fn get_servers(
        state: &AppState,
        app: AppType,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        let all_servers = Self::get_all_servers(state)?;
        let mut result = HashMap::new();

        for (id, server) in all_servers {
            if server.apps.is_enabled_for(&app) {
                result.insert(id, server.server);
            }
        }

        Ok(result)
    }

    /// [已废弃] 设置 MCP 服务器在指定应用的启用状态（兼容旧 API）
    #[deprecated(since = "3.7.0", note = "Use toggle_app instead")]
    pub fn set_enabled(
        state: &AppState,
        app: AppType,
        id: &str,
        enabled: bool,
    ) -> Result<bool, AppError> {
        Self::toggle_app(state, id, app, enabled)?;
        Ok(true)
    }

    /// [已废弃] 同步启用的 MCP 到指定应用（兼容旧 API）
    #[deprecated(since = "3.7.0", note = "Use sync_all_enabled instead")]
    pub fn sync_enabled(state: &AppState, app: AppType) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;

        for server in servers.values() {
            if server.apps.is_enabled_for(&app) {
                Self::sync_server_to_app(state, server, &app)?;
            }
        }

        Ok(())
    }

    /// 从 Claude 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_claude(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp.rs）
        let count = crate::mcp::import_from_claude(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Claude，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.claude = true;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Codex 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_codex(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp.rs）
        let count = crate::mcp::import_from_codex(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Codex，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.codex = true;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Gemini 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_gemini(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp.rs）
        let count = crate::mcp::import_from_gemini(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Gemini，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.gemini = true;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Grok Build 的 `[mcp_servers]` 导入 MCP。
    pub fn import_from_grokbuild(state: &AppState) -> Result<usize, AppError> {
        let mut temp_config = crate::app_config::MultiAppConfig::default();
        let count = crate::mcp::import_from_grokbuild(&mut temp_config)?;
        let mut new_count = 0;

        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.grokbuild = true;
                        merged
                    } else {
                        new_count += 1;
                        server.clone()
                    };
                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save);
                }
            }
        }
        Ok(new_count)
    }

    /// 从 OpenCode 导入 MCP（v3.9.2+ 新增）
    pub fn import_from_opencode(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp/opencode.rs）
        let count = crate::mcp::import_from_opencode(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 OpenCode，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.opencode = true;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Hermes 导入 MCP
    pub fn import_from_hermes(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用导入逻辑（从 mcp/hermes.rs）
        let count = crate::mcp::import_from_hermes(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Hermes，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.hermes = true;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从所有支持 MCP 的应用导入服务器，返回新导入的数量。
    ///
    /// Best-effort：单个应用导入失败（如坏 config.toml）不阻断其余应用；
    /// 全部跑完后若有失败，聚合成一个错误上报——历史实现逐应用
    /// `unwrap_or(0)` 吞错，坏文件只会表现为"导入成功 0 个"，用户
    /// 无从得知哪个应用出了问题。
    pub fn import_from_all_apps(state: &AppState) -> Result<usize, AppError> {
        let mut total = 0;
        let mut failures: Vec<String> = Vec::new();

        let results: [(&str, Result<usize, AppError>); 6] = [
            ("claude", Self::import_from_claude(state)),
            ("codex", Self::import_from_codex(state)),
            ("gemini", Self::import_from_gemini(state)),
            ("grokbuild", Self::import_from_grokbuild(state)),
            ("opencode", Self::import_from_opencode(state)),
            ("hermes", Self::import_from_hermes(state)),
        ];
        for (app, result) in results {
            match result {
                Ok(count) => total += count,
                Err(err) => {
                    log::warn!("从 {app} 导入 MCP 失败: {err}");
                    failures.push(format!("{app}: {err}"));
                }
            }
        }

        if failures.is_empty() {
            Ok(total)
        } else {
            Err(AppError::Message(format!(
                "已导入 {total} 个，部分应用导入失败: {}",
                failures.join("; ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    struct TestHome {
        original: Option<std::ffi::OsString>,
        dir: tempfile::TempDir,
    }

    impl TestHome {
        fn new() -> Self {
            let dir = tempfile::TempDir::new().expect("temp home");
            let original = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self { original, dir }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    struct TransitionHookReset;

    impl Drop for TransitionHookReset {
        fn drop(&mut self) {
            *MCP_TRANSITION_TEST_HOOK
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = None;
        }
    }

    fn test_server(id: &str, command: &str, apps: McpApps) -> McpServer {
        McpServer {
            id: id.to_string(),
            name: id.to_string(),
            server: json!({ "type": "stdio", "command": command }),
            apps,
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        }
    }

    #[test]
    #[serial]
    fn invalid_codex_toml_does_not_commit_toggle_or_delete_to_db() {
        let home = TestHome::new();
        let codex_dir = home.dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let invalid = "[mcp_servers.demo\ncommand = \"echo\"";
        std::fs::write(codex_dir.join("config.toml"), invalid).unwrap();

        let db = Arc::new(crate::database::Database::memory().expect("memory db"));
        let state = AppState::new(db.clone());
        db.save_mcp_server(&test_server(
            "demo",
            "echo",
            McpApps {
                codex: true,
                ..Default::default()
            },
        ))
        .unwrap();

        let toggle = McpService::toggle_app(&state, "demo", AppType::Codex, false);
        assert!(toggle.is_err());
        assert!(db.get_all_mcp_servers().unwrap()["demo"].apps.codex);

        let delete = McpService::delete_server(&state, "demo");
        assert!(delete.is_err());
        assert!(db.get_all_mcp_servers().unwrap().contains_key("demo"));
        assert_eq!(
            std::fs::read_to_string(codex_dir.join("config.toml")).unwrap(),
            invalid
        );
    }

    #[test]
    #[serial]
    fn cross_app_failure_restores_exact_codex_live_snapshot() {
        let home = TestHome::new();
        let codex_dir = home.dir.path().join(".codex");
        let gemini_dir = home.dir.path().join(".gemini");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::create_dir_all(&gemini_dir).unwrap();

        let original_codex = br#"model = "manual-global"
future_top_level = "keep"

[features]
unknown_global_feature = true

[mcp_servers.demo]
type = "stdio"
command = "manual-live"
args = ["--manual"]

[mcp_servers.hand_added]
type = "stdio"
command = "manual-only"
"#;
        let codex_path = codex_dir.join("config.toml");
        std::fs::write(&codex_path, original_codex).unwrap();
        let gemini_path = crate::gemini_config::get_gemini_settings_path();
        std::fs::write(&gemini_path, b"{ invalid gemini json").unwrap();

        let db = Arc::new(crate::database::Database::memory().expect("memory db"));
        let state = AppState::new(db.clone());
        let apps = McpApps {
            codex: true,
            gemini: true,
            ..Default::default()
        };
        db.save_mcp_server(&test_server("demo", "db-old", apps.clone()))
            .unwrap();

        let error = McpService::upsert_server(&state, test_server("demo", "next", apps))
            .expect_err("Gemini parse failure must roll back the earlier Codex write");
        assert!(error.to_string().contains("Gemini") || error.to_string().contains("JSON"));
        assert_eq!(
            db.get_all_mcp_servers().unwrap()["demo"].server["command"],
            "db-old"
        );
        let restored_codex = std::fs::read(&codex_path).unwrap();
        assert_eq!(
            restored_codex, original_codex,
            "rollback must restore the complete pre-transaction file byte-for-byte"
        );
        let restored: toml::Value =
            toml::from_str(std::str::from_utf8(&restored_codex).unwrap()).unwrap();
        assert_eq!(restored["future_top_level"].as_str(), Some("keep"));
        assert_eq!(
            restored["features"]["unknown_global_feature"].as_bool(),
            Some(true)
        );
        assert_eq!(
            restored["mcp_servers"]["demo"]["command"].as_str(),
            Some("manual-live")
        );
        assert_eq!(
            restored["mcp_servers"]["hand_added"]["command"].as_str(),
            Some("manual-only")
        );
    }

    #[test]
    #[serial]
    fn concurrent_toggle_waits_for_upsert_and_reads_committed_db_state() {
        let _home = TestHome::new();
        let _hook_reset = TransitionHookReset;
        let db = Arc::new(crate::database::Database::memory().expect("memory db"));
        db.save_mcp_server(&test_server("serialized", "old", McpApps::default()))
            .unwrap();

        let first_hook = Arc::new(AtomicBool::new(true));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        *MCP_TRANSITION_TEST_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Arc::new({
            let first_hook = first_hook.clone();
            let release_rx = release_rx.clone();
            move |server_id| {
                if server_id == "serialized" && first_hook.swap(false, Ordering::SeqCst) {
                    let _ = entered_tx.send(());
                    let _ = release_rx
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .recv_timeout(Duration::from_secs(5));
                }
            }
        }));

        let first_db = db.clone();
        let first = std::thread::spawn(move || {
            let state = AppState::new(first_db);
            McpService::upsert_server(
                &state,
                test_server("serialized", "committed-first", McpApps::default()),
            )
        });
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first transaction reached controlled point");

        let second_db = db.clone();
        let (second_done_tx, second_done_rx) = mpsc::channel();
        let second = std::thread::spawn(move || {
            let state = AppState::new(second_db);
            let result = McpService::toggle_app(&state, "serialized", AppType::Codex, true);
            let _ = second_done_tx.send(result);
        });
        assert!(matches!(
            second_done_rx.recv_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        release_tx.send(()).unwrap();
        first.join().unwrap().unwrap();
        second_done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("second transaction result")
            .unwrap();
        second.join().unwrap();

        let final_server = db.get_all_mcp_servers().unwrap()["serialized"].clone();
        assert_eq!(final_server.server["command"], "committed-first");
        assert!(final_server.apps.codex);
    }
}

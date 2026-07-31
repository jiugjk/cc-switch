//! Codex MCP 同步和导入模块
//!
//! 包含 Codex 的 MCP 配置管理：
//! - 从 ~/.codex/config.toml 导入
//! - 同步到 ~/.codex/config.toml
//! - JSON 到 TOML 的转换逻辑

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_config::{McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

fn should_sync_codex_mcp() -> bool {
    // Codex 未安装/未初始化时：~/.codex 目录不存在。
    // 按用户偏好：目录缺失时跳过写入/删除，不创建任何文件或目录。
    crate::codex_config::get_codex_config_dir().exists()
}

/// 返回已启用的 MCP 服务器（过滤 enabled==true）
fn collect_enabled_servers(cfg: &McpConfig) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (id, entry) in cfg.servers.iter() {
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        match extract_server_spec(entry) {
            Ok(spec) => {
                out.insert(id.clone(), spec);
            }
            Err(err) => {
                log::warn!("跳过无效的 MCP 条目 '{id}': {err}");
            }
        }
    }
    out
}

/// 将单个 `[mcp_servers.*]` TOML 条目转换为 JSON 服务器规范。
///
/// 未知 type 返回 None（调用方跳过并告警）。转换是有损的：深层非字符串
/// 嵌套表、复杂/混合数组、datetime 字段会被丢弃——与投影方向
/// （`json_server_to_toml_table`）的限制一致。
fn codex_toml_entry_to_json_spec(id: &str, entry_tbl: &toml::value::Table) -> Option<Value> {
    // type 缺省为 stdio
    let typ = entry_tbl
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("stdio");

    // 构建 JSON 规范
    let mut spec = serde_json::Map::new();
    spec.insert("type".into(), json!(typ));

    // 核心字段（需要手动处理的字段）
    let core_fields = match typ {
        "stdio" => vec!["type", "command", "args", "env", "cwd"],
        "http" | "sse" => vec!["type", "url", "http_headers"],
        _ => vec!["type"],
    };

    // 1. 处理核心字段（强类型）
    match typ {
        "stdio" => {
            if let Some(cmd) = entry_tbl.get("command").and_then(|v| v.as_str()) {
                spec.insert("command".into(), json!(cmd));
            }
            if let Some(args) = entry_tbl.get("args").and_then(|v| v.as_array()) {
                let arr = args
                    .iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| json!(s))
                    .collect::<Vec<_>>();
                if !arr.is_empty() {
                    spec.insert("args".into(), serde_json::Value::Array(arr));
                }
            }
            if let Some(cwd) = entry_tbl.get("cwd").and_then(|v| v.as_str()) {
                if !cwd.trim().is_empty() {
                    spec.insert("cwd".into(), json!(cwd));
                }
            }
            if let Some(env_tbl) = entry_tbl.get("env").and_then(|v| v.as_table()) {
                let mut env_json = serde_json::Map::new();
                for (k, v) in env_tbl.iter() {
                    if let Some(sv) = v.as_str() {
                        env_json.insert(k.clone(), json!(sv));
                    }
                }
                if !env_json.is_empty() {
                    spec.insert("env".into(), serde_json::Value::Object(env_json));
                }
            }
        }
        "http" | "sse" => {
            if let Some(url) = entry_tbl.get("url").and_then(|v| v.as_str()) {
                spec.insert("url".into(), json!(url));
            }
            // Read from http_headers (correct Codex format) or headers (legacy) with priority to http_headers
            let headers_tbl = entry_tbl
                .get("http_headers")
                .and_then(|v| v.as_table())
                .or_else(|| entry_tbl.get("headers").and_then(|v| v.as_table()));

            if let Some(headers_tbl) = headers_tbl {
                let mut headers_json = serde_json::Map::new();
                for (k, v) in headers_tbl.iter() {
                    if let Some(sv) = v.as_str() {
                        headers_json.insert(k.clone(), json!(sv));
                    }
                }
                if !headers_json.is_empty() {
                    spec.insert("headers".into(), serde_json::Value::Object(headers_json));
                }
            }
        }
        _ => {
            log::warn!("跳过未知类型 '{typ}' 的 Codex MCP 项 '{id}'");
            return None;
        }
    }

    // 2. 处理扩展字段和其他未知字段（通用 TOML → JSON 转换）
    for (key, toml_val) in entry_tbl.iter() {
        // 跳过已处理的核心字段
        if core_fields.contains(&key.as_str()) {
            continue;
        }

        // 通用 TOML 值到 JSON 值转换
        let json_val = match toml_val {
            toml::Value::String(s) => Some(json!(s)),
            toml::Value::Integer(i) => Some(json!(i)),
            toml::Value::Float(f) => Some(json!(f)),
            toml::Value::Boolean(b) => Some(json!(b)),
            toml::Value::Array(arr) => {
                // 只支持简单类型数组
                let json_arr: Vec<serde_json::Value> = arr
                    .iter()
                    .filter_map(|item| match item {
                        toml::Value::String(s) => Some(json!(s)),
                        toml::Value::Integer(i) => Some(json!(i)),
                        toml::Value::Float(f) => Some(json!(f)),
                        toml::Value::Boolean(b) => Some(json!(b)),
                        _ => None,
                    })
                    .collect();
                if !json_arr.is_empty() {
                    Some(serde_json::Value::Array(json_arr))
                } else {
                    log::debug!("跳过复杂数组字段 '{key}' (TOML → JSON)");
                    None
                }
            }
            toml::Value::Table(tbl) => {
                // 浅层表转为 JSON 对象（仅支持字符串值）
                let mut json_obj = serde_json::Map::new();
                for (k, v) in tbl.iter() {
                    if let Some(s) = v.as_str() {
                        json_obj.insert(k.clone(), json!(s));
                    }
                }
                if !json_obj.is_empty() {
                    Some(serde_json::Value::Object(json_obj))
                } else {
                    log::debug!("跳过复杂对象字段 '{key}' (TOML → JSON)");
                    None
                }
            }
            toml::Value::Datetime(_) => {
                log::debug!("跳过日期时间字段 '{key}' (TOML → JSON)");
                None
            }
        };

        if let Some(val) = json_val {
            spec.insert(key.clone(), val);
            log::debug!("导入扩展字段 '{key}' = {toml_val:?}");
        }
    }

    Some(serde_json::Value::Object(spec))
}

/// 读取 live `~/.codex/config.toml` 中的全部 MCP 服务器定义，返回 id -> JSON 规范。
///
/// 格式支持：
/// - 正确格式：[mcp_servers.*]（Codex 官方标准）
/// - 错误格式：[mcp.servers.*]（容错读取，用于迁移错误写入的配置；同 id 时官方格式优先）
///
/// 文件缺失或为空时返回空表；未知类型或未通过基础校验的条目跳过并告警
/// （与导入语义一致）。导入流程与切换/保存前的 live -> DB 回填共用本函数。
pub fn collect_live_codex_server_specs() -> Result<HashMap<String, Value>, AppError> {
    let text = crate::codex_config::read_and_validate_codex_config_text()?;
    if text.trim().is_empty() {
        return Ok(HashMap::new());
    }

    let root: toml::Table = toml::from_str(&text)
        .map_err(|e| AppError::McpValidation(format!("解析 ~/.codex/config.toml 失败: {e}")))?;

    let mut specs = HashMap::new();
    // legacy [mcp.servers] 先处理，官方 [mcp_servers] 同 id 时覆盖生效
    let legacy_tbl = root
        .get("mcp")
        .and_then(|v| v.as_table())
        .and_then(|tbl| tbl.get("servers"))
        .and_then(|v| v.as_table());
    let official_tbl = root.get("mcp_servers").and_then(|v| v.as_table());
    for servers_tbl in [legacy_tbl, official_tbl].into_iter().flatten() {
        for (id, entry_val) in servers_tbl.iter() {
            let Some(entry_tbl) = entry_val.as_table() else {
                continue;
            };
            let Some(spec) = codex_toml_entry_to_json_spec(id, entry_tbl) else {
                continue;
            };
            // 校验：单项失败继续处理
            if let Err(e) = validate_server_spec(&spec) {
                log::warn!("跳过无效 Codex MCP 项 '{id}': {e}");
                continue;
            }
            specs.insert(id.clone(), spec);
        }
    }
    Ok(specs)
}

/// 从 ~/.codex/config.toml 导入 MCP 到统一结构（v3.7.0+）
///
/// 格式支持：
/// - 正确格式：[mcp_servers.*]（Codex 官方标准）
/// - 错误格式：[mcp.servers.*]（容错读取，用于迁移错误写入的配置）
///
/// 已存在的服务器将启用 Codex 应用，不覆盖其他字段和应用状态
pub fn import_from_codex(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let live_specs = collect_live_codex_server_specs()?;
    if live_specs.is_empty() {
        return Ok(0);
    }

    // 确保新结构存在
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed_total = 0usize;
    for (id, spec_v) in live_specs {
        if let Some(existing) = servers.get_mut(&id) {
            // 已存在：仅启用 Codex 应用
            if !existing.apps.codex {
                existing.apps.codex = true;
                changed_total += 1;
                log::info!("MCP 服务器 '{id}' 已启用 Codex 应用");
            }
        } else {
            // 新建服务器：默认仅启用 Codex
            log::info!("导入新 MCP 服务器 '{id}'");
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec_v,
                    apps: McpApps {
                        claude: false,
                        codex: true,
                        gemini: false,
                        grokbuild: false,
                        opencode: false,
                        hermes: false,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed_total += 1;
        }
    }

    Ok(changed_total)
}

/// 把 JSON 服务器规范经「渲染成 Codex TOML → 重新解析 → 导入方向转换」做一次
/// 有损归一化：投影/导入双向都会丢弃复杂字段（深层非字符串表、混合数组、
/// datetime）、统一 headers/http_headers 命名并补全 type 缺省。回填比较前
/// 双方都过同一条链路，"不同"才真正意味着用户编辑过 live。
fn canonicalize_codex_spec_for_compare(spec: &Value) -> Result<Value, AppError> {
    let table = json_server_to_toml_table(spec)?;
    let mut doc = toml_edit::DocumentMut::new();
    doc["server"] = toml_edit::Item::Table(table);
    let root: toml::Table = toml::from_str(&doc.to_string())
        .map_err(|e| AppError::McpValidation(format!("MCP 规范归一化失败: {e}")))?;
    let entry_tbl = root
        .get("server")
        .and_then(|v| v.as_table())
        .ok_or_else(|| AppError::McpValidation("MCP 规范归一化失败: 缺少条目".into()))?;
    codex_toml_entry_to_json_spec("server", entry_tbl)
        .ok_or_else(|| AppError::McpValidation("MCP 规范归一化失败: 未知类型".into()))
}

/// 判断 DB 存储规范与 live 派生规范在经过 Codex TOML 投影后是否等价。
///
/// 用于切换/保存前的 live -> DB 回填：只有真实的用户编辑（command/cwd/url
/// 等有效字段的差异）才触发回填；round-trip 丢失的字段不算差异。归一化
/// 失败时保守返回 true（视为等价、不回填），避免把转换器的缺陷放大成对
/// DB 行的破坏性覆盖。
pub fn codex_specs_equivalent(db_spec: &Value, live_spec: &Value) -> bool {
    match (
        canonicalize_codex_spec_for_compare(db_spec),
        canonicalize_codex_spec_for_compare(live_spec),
    ) {
        (Ok(db_canonical), Ok(live_canonical)) => db_canonical == live_canonical,
        (Err(err), _) | (_, Err(err)) => {
            log::warn!("MCP 规范归一化失败，保守视为未修改: {err}");
            true
        }
    }
}

/// 将 config.json 中 Codex 的 enabled==true 项以 TOML 形式写入 ~/.codex/config.toml
///
/// 格式策略：
/// - 唯一正确格式：[mcp_servers] 顶层表（Codex 官方标准）
/// - 自动清理错误格式：[mcp.servers]（如果存在）
/// - 读取现有 config.toml；若语法无效则报错，不尝试覆盖
/// - 仅更新 `mcp_servers` 表，保留其它键
/// - 仅写入启用项；无启用项时清理 mcp_servers 表
pub fn sync_enabled_to_codex(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }
    use toml_edit::{Item, Table};

    // 1) 收集启用项（Codex 维度）
    let enabled = collect_enabled_servers(&config.mcp.codex);

    // 2) 读取现有 config.toml 文本；保持无效 TOML 的错误返回（不覆盖文件）
    let base_text = crate::codex_config::read_and_validate_codex_config_text()?;

    // 3) 使用 toml_edit 解析（允许空文件）
    let mut doc = if base_text.trim().is_empty() {
        toml_edit::DocumentMut::default()
    } else {
        base_text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::McpValidation(format!("解析 config.toml 失败: {e}")))?
    };

    // 4) 清理可能存在的错误格式 [mcp.servers]
    if let Some(mcp_item) = doc.get_mut("mcp") {
        if let Some(tbl) = mcp_item.as_table_like_mut() {
            if tbl.contains_key("servers") {
                log::warn!("检测到错误的 MCP 格式 [mcp.servers]，正在清理并迁移到 [mcp_servers]");
                tbl.remove("servers");
            }
        }
    }

    // 5) 构造目标 servers 表（稳定的键顺序）
    if enabled.is_empty() {
        // 无启用项：移除 mcp_servers 表
        doc.as_table_mut().remove("mcp_servers");
    } else {
        // 构建 servers 表
        let mut servers_tbl = Table::new();
        let mut ids: Vec<_> = enabled.keys().cloned().collect();
        ids.sort();
        for id in ids {
            let spec = enabled.get(&id).expect("spec must exist");
            // 复用通用转换函数（已包含扩展字段支持）
            match json_server_to_toml_table(spec) {
                Ok(table) => {
                    servers_tbl[&id[..]] = Item::Table(table);
                }
                Err(err) => {
                    log::error!("跳过无效的 MCP 服务器 '{id}': {err}");
                }
            }
        }
        // 使用唯一正确的格式：[mcp_servers]
        doc["mcp_servers"] = Item::Table(servers_tbl);
    }

    // 6) 写回（仅改 TOML，不触碰 auth.json）；toml_edit 会尽量保留未改区域的注释/空白/顺序
    let new_text = doc.to_string();
    let path = crate::codex_config::get_codex_config_path();
    crate::config::write_text_file(&path, &new_text)?;
    Ok(())
}

/// 将单个 MCP 服务器同步到 Codex live 配置
/// 始终使用 Codex 官方格式 [mcp_servers]，并清理可能存在的错误格式 [mcp.servers]
pub fn sync_single_server_to_codex(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }
    use toml_edit::Item;

    // 读取现有的 config.toml
    let config_path = crate::codex_config::get_codex_config_path();

    let mut doc = if config_path.exists() {
        let content =
            std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;
        // 解析失败必须报错而不是用空文档顶替：写回空文档会把用户
        // config.toml 里的其它段落（model/model_providers/注释等）整体清空
        content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::McpValidation(format!("解析 config.toml 失败: {e}")))?
    } else {
        toml_edit::DocumentMut::new()
    };

    // 清理可能存在的错误格式 [mcp.servers]
    if let Some(mcp_item) = doc.get_mut("mcp") {
        if let Some(tbl) = mcp_item.as_table_like_mut() {
            if tbl.contains_key("servers") {
                log::warn!("检测到错误的 MCP 格式 [mcp.servers]，正在清理并迁移到 [mcp_servers]");
                tbl.remove("servers");
            }
        }
    }

    // 确保 [mcp_servers] 表存在
    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = toml_edit::table();
    }

    // 将 JSON 服务器规范转换为 TOML 表
    let toml_table = json_server_to_toml_table(server_spec)?;

    // 使用唯一正确的格式：[mcp_servers]
    doc["mcp_servers"][id] = Item::Table(toml_table);

    // 写回文件
    let new_text = doc.to_string();
    crate::config::write_text_file(&config_path, &new_text)?;

    Ok(())
}

/// 从 Codex live 配置中移除单个 MCP 服务器
/// 从正确的 [mcp_servers] 表中删除，同时清理可能存在于错误位置 [mcp.servers] 的数据
pub fn remove_server_from_codex(id: &str) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }
    let config_path = crate::codex_config::get_codex_config_path();

    if !config_path.exists() {
        return Ok(()); // 文件不存在，无需删除
    }

    let content =
        std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;

    // 尝试解析现有配置，如果失败则直接返回（无法删除不存在的内容）
    let mut doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(e) => {
            log::warn!("解析 Codex config.toml 失败: {e}，跳过删除操作");
            return Ok(());
        }
    };

    // 从正确的位置删除：[mcp_servers]
    if let Some(mcp_servers) = doc.get_mut("mcp_servers").and_then(|s| s.as_table_mut()) {
        mcp_servers.remove(id);
    }

    // 同时清理可能存在于错误位置的数据：[mcp.servers]（如果存在）
    if let Some(mcp_table) = doc.get_mut("mcp").and_then(|t| t.as_table_mut()) {
        if let Some(servers) = mcp_table.get_mut("servers").and_then(|s| s.as_table_mut()) {
            if servers.remove(id).is_some() {
                log::warn!("从错误的 MCP 格式 [mcp.servers] 中清理了服务器 '{id}'");
            }
        }
    }

    // 写回文件
    let new_text = doc.to_string();
    crate::config::write_text_file(&config_path, &new_text)?;

    Ok(())
}

// ============================================================================
// TOML 转换辅助函数
// ============================================================================

/// 通用 JSON 值到 TOML 值转换器（支持简单类型和浅层嵌套）
///
/// 支持的类型转换：
/// - String → TOML String
/// - Number (i64) → TOML Integer
/// - Number (f64) → TOML Float
/// - Boolean → TOML Boolean
/// - Array[简单类型] → TOML Array
/// - Object → TOML Inline Table (仅字符串值)
///
/// 不支持的类型（返回 None）：
/// - null
/// - 深度嵌套对象
/// - 混合类型数组
fn json_value_to_toml_item(value: &Value, field_name: &str) -> Option<toml_edit::Item> {
    use toml_edit::{Array, InlineTable, Item};

    match value {
        Value::String(s) => Some(toml_edit::value(s.as_str())),

        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml_edit::value(i))
            } else if let Some(f) = n.as_f64() {
                Some(toml_edit::value(f))
            } else {
                log::warn!("跳过字段 '{field_name}': 无法转换的数字类型 {n}");
                None
            }
        }

        Value::Bool(b) => Some(toml_edit::value(*b)),

        Value::Array(arr) => {
            // 只支持简单类型的数组（字符串、数字、布尔）
            let mut toml_arr = Array::default();
            let mut all_same_type = true;

            for item in arr {
                match item {
                    Value::String(s) => toml_arr.push(s.as_str()),
                    Value::Number(n) if n.is_i64() => {
                        if let Some(i) = n.as_i64() {
                            toml_arr.push(i);
                        } else {
                            all_same_type = false;
                            break;
                        }
                    }
                    Value::Number(n) if n.is_f64() => {
                        if let Some(f) = n.as_f64() {
                            toml_arr.push(f);
                        } else {
                            all_same_type = false;
                            break;
                        }
                    }
                    Value::Bool(b) => toml_arr.push(*b),
                    _ => {
                        all_same_type = false;
                        break;
                    }
                }
            }

            if all_same_type && !toml_arr.is_empty() {
                Some(Item::Value(toml_edit::Value::Array(toml_arr)))
            } else {
                log::warn!("跳过字段 '{field_name}': 不支持的数组类型（混合类型或嵌套结构）");
                None
            }
        }

        Value::Object(obj) => {
            // 只支持浅层对象（所有值都是字符串）→ TOML Inline Table
            let mut inline_table = InlineTable::new();
            let mut all_strings = true;

            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    // InlineTable 需要 Value 类型，toml_edit::value() 返回 Item，需要提取内部的 Value
                    inline_table.insert(k, s.into());
                } else {
                    all_strings = false;
                    break;
                }
            }

            if all_strings && !inline_table.is_empty() {
                Some(Item::Value(toml_edit::Value::InlineTable(inline_table)))
            } else {
                log::warn!("跳过字段 '{field_name}': 对象值包含非字符串类型，建议使用子表语法");
                None
            }
        }

        Value::Null => {
            log::debug!("跳过字段 '{field_name}': TOML 不支持 null 值");
            None
        }
    }
}

/// Helper: 将 JSON MCP 服务器规范转换为 toml_edit::Table
///
/// 策略：
/// 1. 核心字段（type, command, args, url, headers, env, cwd）使用强类型处理
/// 2. 扩展字段（timeout、retry 等）通过白名单列表自动转换
/// 3. 其他未知字段使用通用转换器尝试转换
pub(super) fn json_server_to_toml_table(spec: &Value) -> Result<toml_edit::Table, AppError> {
    use toml_edit::{Array, Item, Table};

    let mut t = Table::new();
    let typ = spec.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    t["type"] = toml_edit::value(typ);

    // 定义核心字段（已在下方处理，跳过通用转换）
    let core_fields = match typ {
        "stdio" => vec!["type", "command", "args", "env", "cwd"],
        "http" | "sse" => vec!["type", "url", "http_headers"],
        _ => vec!["type"],
    };

    // 定义扩展字段白名单（Codex 常见可选字段）
    let extended_fields = [
        // 通用字段
        "timeout",
        "timeout_ms",
        "startup_timeout_ms",
        "startup_timeout_sec",
        "connection_timeout",
        "read_timeout",
        "debug",
        "log_level",
        "disabled",
        // stdio 特有
        "shell",
        "encoding",
        "working_dir",
        "restart_on_exit",
        "max_restart_count",
        // http/sse 特有
        "retry_count",
        "max_retry_attempts",
        "retry_delay",
        "cache_tools_list",
        "verify_ssl",
        "insecure",
        "proxy",
    ];

    // 1. 处理核心字段（强类型）
    match typ {
        "stdio" => {
            let cmd = spec.get("command").and_then(|v| v.as_str()).unwrap_or("");
            t["command"] = toml_edit::value(cmd);

            if let Some(args) = spec.get("args").and_then(|v| v.as_array()) {
                let mut arr_v = Array::default();
                for a in args.iter().filter_map(|x| x.as_str()) {
                    arr_v.push(a);
                }
                if !arr_v.is_empty() {
                    t["args"] = Item::Value(toml_edit::Value::Array(arr_v));
                }
            }

            if let Some(cwd) = spec.get("cwd").and_then(|v| v.as_str()) {
                if !cwd.trim().is_empty() {
                    t["cwd"] = toml_edit::value(cwd);
                }
            }

            if let Some(env) = spec.get("env").and_then(|v| v.as_object()) {
                let mut env_tbl = Table::new();
                for (k, v) in env.iter() {
                    if let Some(s) = v.as_str() {
                        env_tbl[&k[..]] = toml_edit::value(s);
                    }
                }
                if !env_tbl.is_empty() {
                    t["env"] = Item::Table(env_tbl);
                }
            }
        }
        "http" | "sse" => {
            let url = spec.get("url").and_then(|v| v.as_str()).unwrap_or("");
            t["url"] = toml_edit::value(url);

            if let Some(headers) = spec.get("headers").and_then(|v| v.as_object()) {
                let mut h_tbl = Table::new();
                for (k, v) in headers.iter() {
                    if let Some(s) = v.as_str() {
                        h_tbl[&k[..]] = toml_edit::value(s);
                    }
                }
                if !h_tbl.is_empty() {
                    t["http_headers"] = Item::Table(h_tbl);
                }
            }
        }
        _ => {}
    }

    // 2. 处理扩展字段和其他未知字段
    if let Some(obj) = spec.as_object() {
        for (key, value) in obj {
            // 跳过已处理的核心字段
            if core_fields.contains(&key.as_str()) {
                continue;
            }

            // 尝试使用通用转换器
            if let Some(toml_item) = json_value_to_toml_item(value, key) {
                t[&key[..]] = toml_item;

                // 记录扩展字段的处理
                if extended_fields.contains(&key.as_str()) {
                    log::debug!("已转换扩展字段 '{key}' = {value:?}");
                } else {
                    log::info!("已转换自定义字段 '{key}' = {value:?}");
                }
            }
        }
    }

    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    struct TestHome {
        original: Option<std::ffi::OsString>,
        #[allow(dead_code)]
        dir: TempDir,
    }

    impl TestHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("temp dir");
            let original = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self { original, dir }
        }

        fn write_codex_config(&self, text: &str) {
            let codex_dir = self.dir.path().join(".codex");
            std::fs::create_dir_all(&codex_dir).expect("create .codex dir");
            std::fs::write(codex_dir.join("config.toml"), text).expect("write config.toml");
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

    #[test]
    #[serial]
    fn collect_live_codex_server_specs_parses_stdio_http_and_legacy() {
        let home = TestHome::new();
        home.write_codex_config(
            r#"model = "gpt-5.5"

[mcp_servers.node_repl]
type = "stdio"
command = "node"
args = ["repl.js"]
cwd = "/opt/new-install"
startup_timeout_ms = 5000

[mcp_servers.node_repl.env]
NODE_ENV = "production"

[mcp_servers.search]
type = "http"
url = "https://mcp.example/sse"

[mcp_servers.search.http_headers]
Authorization = "Bearer token"

[mcp.servers.legacy-echo]
command = "echo"
"#,
        );

        let specs = collect_live_codex_server_specs().expect("collect specs");

        assert_eq!(specs.len(), 3, "all three live entries should be collected");
        let node = &specs["node_repl"];
        assert_eq!(node["type"], "stdio");
        assert_eq!(node["command"], "node");
        assert_eq!(node["args"][0], "repl.js");
        assert_eq!(node["cwd"], "/opt/new-install");
        assert_eq!(node["env"]["NODE_ENV"], "production");
        assert_eq!(node["startup_timeout_ms"], 5000, "extended field imported");
        let search = &specs["search"];
        assert_eq!(search["type"], "http");
        assert_eq!(search["url"], "https://mcp.example/sse");
        assert_eq!(
            search["headers"]["Authorization"], "Bearer token",
            "http_headers should normalize to the unified headers key"
        );
        let legacy = &specs["legacy-echo"];
        assert_eq!(legacy["type"], "stdio", "legacy entries default to stdio");
        assert_eq!(legacy["command"], "echo");
    }

    #[test]
    #[serial]
    fn collect_live_codex_server_specs_handles_missing_file_and_invalid_entries() {
        let home = TestHome::new();

        // 无 ~/.codex/config.toml：返回空表而非报错
        let specs = collect_live_codex_server_specs().expect("collect with no config");
        assert!(
            specs.is_empty(),
            "missing config.toml should yield no specs"
        );

        // 无效条目（空 command）跳过，有效条目仍被收集
        home.write_codex_config(
            r#"[mcp_servers.bad]
type = "stdio"
command = ""

[mcp_servers.good]
type = "stdio"
command = "echo"
"#,
        );
        let specs = collect_live_codex_server_specs().expect("collect specs");
        assert_eq!(specs.len(), 1, "invalid entry must be skipped");
        assert!(specs.contains_key("good"));
    }

    #[test]
    fn codex_specs_equivalent_neutralizes_lossy_round_trip() {
        // DB 规范里带有投影方向会丢弃的复杂字段（非字符串嵌套表），且使用
        // 统一的 headers 命名；live 派生规范经过 TOML round-trip 后没有这些
        // 差异——两者必须判等，否则每次切换都会产生虚假回填。
        let db_spec = json!({
            "type": "http",
            "url": "https://mcp.example",
            "headers": { "Authorization": "Bearer token" },
            "nested": { "count": 1 }
        });
        let live_spec = json!({
            "type": "http",
            "url": "https://mcp.example",
            "headers": { "Authorization": "Bearer token" }
        });
        assert!(
            codex_specs_equivalent(&db_spec, &live_spec),
            "lossy round-trip differences must not count as user edits"
        );
    }

    #[test]
    fn codex_specs_equivalent_treats_missing_type_as_stdio() {
        let db_spec = json!({ "command": "echo" });
        let live_spec = json!({ "type": "stdio", "command": "echo" });
        assert!(codex_specs_equivalent(&db_spec, &live_spec));
    }

    #[test]
    fn codex_specs_equivalent_detects_real_edits() {
        let db_spec = json!({ "type": "stdio", "command": "node", "cwd": "/opt/old" });
        let live_spec = json!({ "type": "stdio", "command": "node", "cwd": "/opt/new" });
        assert!(
            !codex_specs_equivalent(&db_spec, &live_spec),
            "a changed cwd is a real user edit"
        );
    }
}

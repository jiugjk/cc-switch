use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{get_app_config_dir, get_home_dir, write_text_file};
use crate::error::AppError;
use crate::provider::Provider;

pub const DEFAULT_MODEL: &str = "grok-4.5";
pub const DEFAULT_API_BACKEND: &str = "responses";
pub const DEFAULT_CONTEXT_WINDOW: i64 = 500_000;
const MAX_GROK_CONFIG_BACKUPS: usize = 10;
const GROK_CONFIG_BACKUP_PREFIX: &str = "grok-config-";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokConfigLocation {
    pub path: String,
    pub directory: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokConfigBackup {
    pub filename: String,
    pub path: String,
    pub created_at: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokModelConfig {
    pub profile: String,
    pub model: String,
    pub base_url: String,
    pub name: String,
    pub api_key: Option<String>,
    pub env_key: Option<String>,
    pub api_backend: String,
    pub context_window: i64,
}

fn non_empty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn absolute_environment_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// Resolve the Grok Build live configuration using the same environment
/// contract as Grok-oriented tooling: an explicit `GROK_CONFIG` file wins,
/// then `GROK_HOME`, then the CC Switch directory override, then `~/.grok`.
pub fn get_grok_config_location() -> GrokConfigLocation {
    let (path, source) = if let Some(path) = non_empty_env_path("GROK_CONFIG") {
        (absolute_environment_path(path), "GROK_CONFIG")
    } else if let Some(directory) = non_empty_env_path("GROK_HOME") {
        (
            absolute_environment_path(directory).join("config.toml"),
            "GROK_HOME",
        )
    } else if let Some(directory) = crate::settings::get_grok_override_dir() {
        (directory.join("config.toml"), "settings")
    } else {
        (get_home_dir().join(".grok").join("config.toml"), "default")
    };
    let directory = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    GrokConfigLocation {
        path: path.to_string_lossy().to_string(),
        directory: directory.to_string_lossy().to_string(),
        source: source.to_string(),
    }
}

/// Grok Build configuration directory.
pub fn get_grok_config_dir() -> PathBuf {
    PathBuf::from(get_grok_config_location().directory)
}

/// Grok Build live configuration path.
pub fn get_grok_config_path() -> PathBuf {
    PathBuf::from(get_grok_config_location().path)
}

pub fn get_grok_config_backup_dir() -> PathBuf {
    get_app_config_dir().join("grok-config-backups")
}

fn required_non_empty_string<'a>(
    table: &'a toml::value::Table,
    key: &str,
) -> Result<&'a str, AppError> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.field.missing",
                format!("Grok Build 配置缺少有效的 {key} 字段"),
                format!("Grok Build configuration is missing a valid {key} field"),
            )
        })
}

fn optional_non_empty_string(table: &toml::value::Table, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// Syntax-only validation for a Grok Build config document (empty allowed).
///
/// 官方条目走 Grok CLI 自带的 xAI OAuth 登录，config.toml 不需要（通常也没有）
/// 自定义模型表：空文档合法，非空只要求 TOML 语法合法。live 层的读写与官方
/// 快照校验都用它；"必须有完整自定义模型表"的强校验见 `validate_config_toml`。
pub fn validate_config_toml_syntax(config_toml: &str) -> Result<(), AppError> {
    if config_toml.trim().is_empty() {
        return Ok(());
    }
    config_toml
        .parse::<toml::Value>()
        .map(|_| ())
        .map_err(|error| {
            AppError::localized(
                "provider.grokbuild.config.invalid_toml",
                format!("Grok Build config.toml 格式错误: {error}"),
                format!("Invalid Grok Build config.toml: {error}"),
            )
        })
}

/// Whether a live config document represents the official login state.
///
/// 官方态 = 语法合法且没有任何 provider-owned 模型字段。允许 `[models]`
/// 中的未来全局键和 `[mcp_servers]` 等其它内容，但 `models.default`、
/// `models.web_search`、`endpoints.models_base_url`、`subagents` 或任一
/// `[model.*]` 都表示自定义供应商态。语法不合法同样返回 false。
pub fn is_official_live_config(config_toml: &str) -> bool {
    let Ok(document) = config_toml.parse::<toml::Value>() else {
        return false;
    };
    document.as_table().is_some_and(|root| {
        let has_endpoint = root
            .get("endpoints")
            .and_then(toml::Value::as_table)
            .is_some_and(|table| table.contains_key("models_base_url"));
        let has_model_selection = root
            .get("models")
            .and_then(toml::Value::as_table)
            .is_some_and(|table| table.contains_key("default") || table.contains_key("web_search"));
        !has_endpoint
            && !has_model_selection
            && !root.contains_key("subagents")
            && !root.contains_key("model")
    })
}

/// Validate the provider-owned Grok Build TOML document.
pub fn validate_config_toml(config_toml: &str) -> Result<(), AppError> {
    let document = config_toml.parse::<toml::Value>().map_err(|error| {
        AppError::localized(
            "provider.grokbuild.config.invalid_toml",
            format!("Grok Build config.toml 格式错误: {error}"),
            format!("Invalid Grok Build config.toml: {error}"),
        )
    })?;

    let root = document.as_table().ok_or_else(|| {
        AppError::localized(
            "provider.grokbuild.config.not_table",
            "Grok Build 配置必须是 TOML 表结构",
            "Grok Build configuration must be a TOML table",
        )
    })?;
    let models = root
        .get("models")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.models.missing",
                "Grok Build 配置缺少 [models]",
                "Grok Build configuration is missing [models]",
            )
        })?;
    let default_model = required_non_empty_string(models, "default")?;
    let model_entries = root
        .get("model")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.model.missing",
                "Grok Build 配置缺少 [model.<name>]",
                "Grok Build configuration is missing [model.<name>]",
            )
        })?;
    let selected_model = model_entries
        .get(default_model)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.default_model.missing",
                format!("Grok Build 配置缺少 [model.\"{default_model}\"]"),
                format!("Grok Build configuration is missing [model.\"{default_model}\"]"),
            )
        })?;

    required_non_empty_string(selected_model, "model")?;
    required_non_empty_string(selected_model, "base_url")?;
    required_non_empty_string(selected_model, "name")?;
    if optional_non_empty_string(selected_model, "api_key").is_none()
        && optional_non_empty_string(selected_model, "env_key").is_none()
    {
        return Err(AppError::localized(
            "provider.grokbuild.credentials.missing",
            "Grok Build 配置缺少有效的 api_key 或 env_key 字段",
            "Grok Build configuration is missing a valid api_key or env_key field",
        ));
    }
    required_non_empty_string(selected_model, "api_backend")?;

    selected_model
        .get("context_window")
        .and_then(toml::Value::as_integer)
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.context_window.invalid",
                "Grok Build context_window 必须是正整数",
                "Grok Build context_window must be a positive integer",
            )
        })?;

    Ok(())
}

fn parse_edit_document(config_toml: &str) -> Result<toml_edit::DocumentMut, AppError> {
    config_toml
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| {
            AppError::localized(
                "provider.grokbuild.config.invalid_toml",
                format!("Grok Build config.toml 格式错误: {error}"),
                format!("Invalid Grok Build config.toml: {error}"),
            )
        })
}

fn table_like_mut<'a>(
    document: &'a mut toml_edit::DocumentMut,
    section: &str,
) -> Result<&'a mut dyn toml_edit::TableLike, AppError> {
    if document.get(section).is_none() {
        document[section] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    document
        .get_mut(section)
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| {
            AppError::localized(
                "grokBuild.sectionNotTable",
                format!("Grok Build 配置中的 {section} 必须是 TOML 表"),
                format!("Grok Build configuration section {section} must be a TOML table"),
            )
        })
}

fn set_table_value(
    document: &mut toml_edit::DocumentMut,
    section: &str,
    key: &str,
    value: toml_edit::Item,
) -> Result<(), AppError> {
    table_like_mut(document, section)?.insert(key, value);
    Ok(())
}

fn remove_table_key(document: &mut toml_edit::DocumentMut, section: &str, key: &str) {
    let remove_section = document
        .get_mut(section)
        .and_then(toml_edit::Item::as_table_like_mut)
        .is_some_and(|table| {
            table.remove(key);
            table.is_empty()
        });
    if remove_section {
        document.as_table_mut().remove(section);
    }
}

fn remove_provider_owned_fields(document: &mut toml_edit::DocumentMut) {
    remove_table_key(document, "endpoints", "models_base_url");
    remove_table_key(document, "models", "default");
    remove_table_key(document, "models", "web_search");
    document.as_table_mut().remove("subagents");
    document.as_table_mut().remove("model");
}

fn copy_table_key(
    target: &mut toml_edit::DocumentMut,
    source: &toml_edit::DocumentMut,
    section: &str,
    key: &str,
) {
    if let Some(item) = source.get(section).and_then(|item| item.get(key)) {
        if target.get(section).is_none() {
            target[section] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        if let Some(table) = target
            .get_mut(section)
            .and_then(toml_edit::Item::as_table_like_mut)
        {
            table.insert(key, item.clone());
        }
    }
}

/// Extract only the Grok provider profile from a full live config.
///
/// Provider snapshots own endpoint/model selection, subagent model selection,
/// and complete `[model.*]` tables. Everything else remains live-global and is
/// deliberately excluded so switches cannot pin telemetry, harness, UI, or
/// future Grok settings to an individual provider.
pub fn extract_provider_profile_config_text(config_toml: &str) -> Result<String, AppError> {
    let source = parse_edit_document(config_toml)?;
    let mut profile = toml_edit::DocumentMut::new();
    copy_table_key(&mut profile, &source, "endpoints", "models_base_url");
    copy_table_key(&mut profile, &source, "models", "default");
    copy_table_key(&mut profile, &source, "models", "web_search");
    if let Some(subagents) = source.get("subagents") {
        profile["subagents"] = subagents.clone();
    }
    if let Some(models) = source.get("model") {
        profile["model"] = models.clone();
    }
    Ok(profile.to_string())
}

/// Merge a provider-owned Grok profile into the current full live config while
/// preserving every global/unrecognized section and key.
pub fn merge_provider_profile_config_text(
    live_config_toml: &str,
    provider_config_toml: &str,
) -> Result<String, AppError> {
    validate_config_toml(provider_config_toml)?;
    let mut live = parse_edit_document(live_config_toml)?;
    let provider = parse_edit_document(provider_config_toml)?;
    remove_provider_owned_fields(&mut live);
    copy_table_key(&mut live, &provider, "endpoints", "models_base_url");
    copy_table_key(&mut live, &provider, "models", "default");
    copy_table_key(&mut live, &provider, "models", "web_search");
    if let Some(subagents) = provider.get("subagents") {
        live["subagents"] = subagents.clone();
    }
    if let Some(models) = provider.get("model") {
        live["model"] = models.clone();
    }
    Ok(live.to_string())
}

/// Remove provider-owned Grok fields while retaining global config. This is
/// the official-login transition: the absence of `[models]`/`[model.*]` lets
/// Grok use its own OAuth state without erasing telemetry, harness, UI, or MCP.
pub fn remove_provider_profile_config_text(config_toml: &str) -> Result<String, AppError> {
    let mut document = parse_edit_document(config_toml)?;
    remove_provider_owned_fields(&mut document);
    Ok(document.to_string())
}

pub fn extract_provider_profile_from_settings(settings: &mut Value) -> Result<(), AppError> {
    let Some(config_toml) = settings.get("config").and_then(Value::as_str) else {
        return Ok(());
    };
    let profile = extract_provider_profile_config_text(config_toml)?;
    if let Some(object) = settings.as_object_mut() {
        object.insert("config".to_string(), Value::String(profile));
    }
    Ok(())
}

/// Apply the opt-in privacy preset to a config draft. This function does not
/// write the file; the UI shows the resulting TOML and requires an explicit
/// Save so users can inspect the exact changes first.
pub fn apply_privacy_protection_config_text(config_toml: &str) -> Result<String, AppError> {
    let mut document = parse_edit_document(config_toml)?;
    set_table_value(
        &mut document,
        "features",
        "telemetry",
        toml_edit::value(false),
    )?;
    set_table_value(
        &mut document,
        "telemetry",
        "trace_upload",
        toml_edit::value(false),
    )?;
    set_table_value(
        &mut document,
        "telemetry",
        "mixpanel_enabled",
        toml_edit::value(false),
    )?;
    set_table_value(
        &mut document,
        "harness",
        "disable_codebase_upload",
        toml_edit::value(true),
    )?;
    Ok(document.to_string())
}

pub fn extract_model_config(config_toml: &str) -> Option<GrokModelConfig> {
    let document = config_toml.parse::<toml::Value>().ok()?;
    let root = document.as_table()?;
    let default_model = root
        .get("models")?
        .as_table()?
        .get("default")?
        .as_str()?
        .trim();
    let selected_model = root
        .get("model")?
        .as_table()?
        .get(default_model)?
        .as_table()?;
    Some(GrokModelConfig {
        profile: default_model.to_string(),
        model: selected_model.get("model")?.as_str()?.trim().to_string(),
        base_url: selected_model
            .get("base_url")?
            .as_str()?
            .trim_end_matches('/')
            .to_string(),
        name: selected_model.get("name")?.as_str()?.trim().to_string(),
        api_key: optional_non_empty_string(selected_model, "api_key"),
        env_key: optional_non_empty_string(selected_model, "env_key"),
        api_backend: selected_model
            .get("api_backend")?
            .as_str()?
            .trim()
            .to_string(),
        context_window: selected_model.get("context_window")?.as_integer()?,
    })
}

pub fn extract_credentials(config_toml: &str) -> Option<(String, String)> {
    let config = extract_model_config(config_toml)?;
    // Credentials only come from two explicit, config-declared sources:
    //   1. an inline `api_key`, or
    //   2. the process env var named by `env_key`.
    //
    // Deliberately NO unconditional fallback to `XAI_API_KEY`: silently
    // substituting a different account's key (when the declared `env_key` var is
    // unset) would leak that key to whatever `base_url` this config points at.
    // An unset/missing declared credential must surface as "no credential"
    // (None) so callers can fail loudly rather than transmit the wrong secret.
    let api_key = config.api_key.or_else(|| {
        config
            .env_key
            .as_deref()
            .and_then(|key| std::env::var(key).ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })?;
    Some((config.base_url, api_key))
}

pub fn extract_inline_api_key(config_toml: &str) -> Option<String> {
    extract_model_config(config_toml)?.api_key
}

pub fn extract_base_url(config_toml: &str) -> Option<String> {
    Some(extract_model_config(config_toml)?.base_url)
}

fn update_selected_model_string(
    config_toml: &str,
    field: &str,
    value: &str,
) -> Result<String, AppError> {
    let mut document = config_toml
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| {
            AppError::localized(
                "provider.grokbuild.config.invalid_toml",
                format!("Grok Build config.toml 格式错误: {error}"),
                format!("Invalid Grok Build config.toml: {error}"),
            )
        })?;
    let default_model = document
        .get("models")
        .and_then(|item| item.get("default"))
        .and_then(toml_edit::Item::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.default_model.missing",
                "Grok Build 配置缺少 models.default",
                "Grok Build configuration is missing models.default",
            )
        })?
        .to_string();

    let selected_model = document
        .get_mut("model")
        .and_then(|item| item.get_mut(&default_model))
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.default_model.missing",
                format!("Grok Build 配置缺少 [model.\"{default_model}\"]"),
                format!("Grok Build configuration is missing [model.\"{default_model}\"]"),
            )
        })?;
    selected_model.insert(field, toml_edit::value(value));
    Ok(document.to_string())
}

pub fn apply_proxy_takeover(
    config_toml: &str,
    proxy_base_url: &str,
    token_placeholder: &str,
) -> Result<String, AppError> {
    validate_config_toml(config_toml)?;
    let mut document = parse_edit_document(config_toml)?;
    let models = document
        .get_mut("model")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.model.missing",
                "Grok Build 配置缺少 [model.<name>]",
                "Grok Build configuration is missing [model.<name>]",
            )
        })?;

    let mut takeover_count = 0usize;
    for (_, item) in models.iter_mut() {
        let Some(model) = item.as_table_like_mut() else {
            continue;
        };
        model.insert("base_url", toml_edit::value(proxy_base_url));
        model.insert("api_key", toml_edit::value(token_placeholder));
        // The Grok proxy exposes the Responses protocol for every profile.
        // Forcing this value avoids routing non-default/subagent profiles to a
        // chat-completions endpoint that this adapter intentionally does not expose.
        model.insert("api_backend", toml_edit::value(DEFAULT_API_BACKEND));
        takeover_count += 1;
    }
    if takeover_count == 0 {
        return Err(AppError::localized(
            "provider.grokbuild.model.missing",
            "Grok Build 配置缺少可接管的 [model.<name>]",
            "Grok Build configuration has no [model.<name>] entry to take over",
        ));
    }
    set_table_value(
        &mut document,
        "endpoints",
        "models_base_url",
        toml_edit::value(proxy_base_url),
    )?;
    Ok(document.to_string())
}

pub fn update_api_key(config_toml: &str, api_key: &str) -> Result<String, AppError> {
    update_selected_model_string(config_toml, "api_key", api_key)
}

#[allow(dead_code)]
pub fn has_proxy_placeholder(config_toml: &str, token_placeholder: &str) -> bool {
    has_proxy_credential(config_toml, |api_key| api_key == token_placeholder)
}

pub fn has_proxy_credential(config_toml: &str, is_proxy_credential: impl Fn(&str) -> bool) -> bool {
    config_toml
        .parse::<toml::Value>()
        .ok()
        .and_then(|document| {
            document
                .get("model")
                .and_then(toml::Value::as_table)
                .cloned()
        })
        .is_some_and(|models| {
            models.values().any(|model| {
                model
                    .as_table()
                    .and_then(|table| table.get("api_key"))
                    .and_then(toml::Value::as_str)
                    .is_some_and(&is_proxy_credential)
            })
        })
}

/// Emergency cleanup used only when both the takeover restore backup and the
/// provider SSOT are unavailable. Remove every field owned by takeover so no
/// model can keep using a dead local route or the synthetic credential.
#[allow(dead_code)]
pub fn remove_proxy_takeover_fields(
    config_toml: &str,
    token_placeholder: &str,
    is_proxy_url: impl Fn(&str) -> bool,
) -> Result<String, AppError> {
    remove_proxy_takeover_fields_if(
        config_toml,
        |api_key| api_key == token_placeholder,
        is_proxy_url,
    )
}

pub fn remove_proxy_takeover_fields_if(
    config_toml: &str,
    is_proxy_credential: impl Fn(&str) -> bool,
    is_proxy_url: impl Fn(&str) -> bool,
) -> Result<String, AppError> {
    let mut document = parse_edit_document(config_toml)?;
    let mut has_takeover_credential = false;
    if let Some(models) = document
        .get_mut("model")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        for (_, item) in models.iter_mut() {
            let Some(model) = item.as_table_like_mut() else {
                continue;
            };
            let model_has_takeover_credential = model
                .get("api_key")
                .and_then(toml_edit::Item::as_str)
                .is_some_and(&is_proxy_credential);
            has_takeover_credential |= model_has_takeover_credential;
            if model_has_takeover_credential {
                model.remove("api_key");
            }
            if model
                .get("base_url")
                .and_then(toml_edit::Item::as_str)
                .is_some_and(|url| model_has_takeover_credential || is_proxy_url(url))
            {
                model.remove("base_url");
            }
        }
    }

    let remove_endpoints = document
        .get_mut("endpoints")
        .and_then(toml_edit::Item::as_table_like_mut)
        .is_some_and(|endpoints| {
            if endpoints
                .get("models_base_url")
                .and_then(toml_edit::Item::as_str)
                .is_some_and(|url| has_takeover_credential || is_proxy_url(url))
            {
                endpoints.remove("models_base_url");
            }
            endpoints.is_empty()
        });
    if remove_endpoints {
        document.as_table_mut().remove("endpoints");
    }

    Ok(document.to_string())
}

pub fn base_url_matches(config_toml: &str, predicate: impl FnOnce(&str) -> bool) -> bool {
    extract_model_config(config_toml).is_some_and(|config| predicate(&config.base_url))
}

/// Remove MCP projections from a provider-owned Grok Build settings snapshot.
/// MCP servers are owned by the database and projected into live config.toml.
/// The strip is lossless for user data: switch/save paths run
/// `McpService::backfill_live_edits_for_app` before rewriting live, so manual
/// `[mcp_servers.*]` edits are absorbed into the DB first; this only prevents
/// deleted servers from resurrecting via the provider snapshot.
pub fn strip_grok_mcp_servers_from_settings(settings: &mut Value) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(());
    };
    if !config_text.contains("mcp") {
        return Ok(());
    }

    let mut document = config_text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| AppError::Message(format!("Invalid Grok Build config.toml: {error}")))?;
    let mut changed = document.as_table_mut().remove("mcp_servers").is_some();
    if let Some(mcp_table) = document
        .get_mut("mcp")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        if mcp_table.remove("servers").is_some() {
            changed = true;
        }
        if mcp_table.is_empty() {
            document.as_table_mut().remove("mcp");
        }
    }

    if changed {
        if let Some(object) = settings.as_object_mut() {
            object.insert("config".to_string(), Value::String(document.to_string()));
        }
    }
    Ok(())
}

/// Read the live `~/.grok/config.toml` as a provider settings snapshot.
///
/// 只做 TOML 语法校验：live 处于官方态（无自定义模型表）时同样需要能被
/// 读取，供切换回填与界面展示使用。需要"完整自定义模型配置"的导入路径
/// 由调用方自行叠加 `validate_config_toml`。
pub fn read_grok_live_settings() -> Result<Value, AppError> {
    let path = get_grok_config_path();
    if !path.exists() {
        return Err(AppError::localized(
            "grokbuild.config.missing",
            "Grok Build 配置文件不存在",
            "Grok Build configuration file not found",
        ));
    }

    let config = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    validate_config_toml_syntax(&config)?;
    Ok(json!({ "config": config }))
}

fn backup_current_grok_config(path: &Path, next_config: &str) -> Result<Option<PathBuf>, AppError> {
    if !path.exists() {
        return Ok(None);
    }
    let current = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    if current == next_config {
        return Ok(None);
    }

    let backup_dir = get_grok_config_backup_dir();
    ensure_secure_backup_dir(&backup_dir)?;
    let filename = format!(
        "{}{}.toml",
        GROK_CONFIG_BACKUP_PREFIX,
        Utc::now().format("%Y%m%d_%H%M%S_%9f")
    );
    let backup_path = backup_dir.join(filename);
    write_secure_backup_file(&backup_path, &current)?;

    let backups = list_grok_config_backups()?;
    for stale in backups.into_iter().skip(MAX_GROK_CONFIG_BACKUPS) {
        let stale_path = backup_dir.join(stale.filename);
        fs::remove_file(&stale_path).map_err(|error| AppError::io(&stale_path, error))?;
    }
    Ok(Some(backup_path))
}

fn ensure_secure_backup_dir(path: &Path) -> Result<(), AppError> {
    fs::create_dir_all(path).map_err(|error| AppError::io(path, error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| AppError::io(path, error))?;
    }
    Ok(())
}

fn write_secure_backup_file(path: &Path, content: &str) -> Result<(), AppError> {
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| AppError::io(path, error))?
    };
    #[cfg(not(unix))]
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| AppError::io(path, error))?;

    file.write_all(content.as_bytes())
        .map_err(|error| AppError::io(path, error))?;
    file.flush().map_err(|error| AppError::io(path, error))?;
    Ok(())
}

fn validated_backup_path(filename: &str) -> Result<PathBuf, AppError> {
    let candidate = Path::new(filename);
    let is_plain_filename = candidate
        .file_name()
        .is_some_and(|value| value == candidate.as_os_str());
    if !is_plain_filename
        || !filename.starts_with(GROK_CONFIG_BACKUP_PREFIX)
        || !filename.ends_with(".toml")
    {
        return Err(AppError::Config(
            "Invalid Grok Build backup filename".to_string(),
        ));
    }
    Ok(get_grok_config_backup_dir().join(filename))
}

pub fn list_grok_config_backups() -> Result<Vec<GrokConfigBackup>, AppError> {
    let backup_dir = get_grok_config_backup_dir();
    if !backup_dir.exists() {
        return Ok(Vec::new());
    }
    ensure_secure_backup_dir(&backup_dir)?;

    let mut backups = Vec::new();
    let entries = fs::read_dir(&backup_dir).map_err(|error| AppError::io(&backup_dir, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| AppError::io(&backup_dir, error))?;
        let path = entry.path();
        let filename = entry.file_name().to_string_lossy().to_string();
        if !entry
            .file_type()
            .map_err(|error| AppError::io(&path, error))?
            .is_file()
            || !filename.starts_with(GROK_CONFIG_BACKUP_PREFIX)
            || !filename.ends_with(".toml")
        {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| AppError::io(&path, error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|error| AppError::io(&path, error))?;
        }
        let modified = metadata
            .modified()
            .ok()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now);
        backups.push(GrokConfigBackup {
            filename,
            path: path.to_string_lossy().to_string(),
            created_at: modified.to_rfc3339(),
            size_bytes: metadata.len(),
        });
    }
    backups.sort_by(|left, right| right.filename.cmp(&left.filename));
    Ok(backups)
}

pub fn read_grok_config_backup(filename: &str) -> Result<String, AppError> {
    let backup_path = validated_backup_path(filename)?;
    let config =
        fs::read_to_string(&backup_path).map_err(|error| AppError::io(&backup_path, error))?;
    validate_config_toml_syntax(&config)?;
    Ok(config)
}

pub fn restore_grok_config_backup(filename: &str) -> Result<String, AppError> {
    let config = read_grok_config_backup(filename)?;
    write_grok_live_settings(&json!({ "config": config }))?;
    Ok(config)
}

pub fn delete_grok_config_backup(filename: &str) -> Result<bool, AppError> {
    let backup_path = validated_backup_path(filename)?;
    if !backup_path.exists() {
        return Ok(false);
    }
    fs::remove_file(&backup_path).map_err(|error| AppError::io(&backup_path, error))?;
    Ok(true)
}

pub fn write_grok_provider_live(provider: &Provider) -> Result<(), AppError> {
    let settings = provider.settings_config.as_object().ok_or_else(|| {
        AppError::localized(
            "provider.grokbuild.settings.not_object",
            "Grok Build 配置必须是 JSON 对象",
            "Grok Build configuration must be a JSON object",
        )
    })?;
    let config = settings
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.config.missing",
                "Grok Build 配置缺少 config 字段",
                "Grok Build configuration is missing the config field",
            )
        })?;

    let path = get_grok_config_path();
    let live_config = if path.exists() {
        fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?
    } else {
        String::new()
    };
    validate_config_toml_syntax(&live_config)?;

    // Provider switches replace only provider-owned endpoint/model fields.
    // Global flags, MCP projections, and unknown future Grok settings remain
    // attached to the live installation rather than following a provider.
    let merged = if provider.category.as_deref() == Some("official") {
        remove_provider_profile_config_text(&live_config)?
    } else {
        merge_provider_profile_config_text(&live_config, config)?
    };
    write_grok_live_settings(&json!({ "config": merged }))
}

/// Raw live-file writer, mirroring `read_grok_live_settings` (syntax-only).
///
/// 代理接管的备份/恢复也走这里：官方态 live（无自定义模型表）必须可以
/// 原样写回。完整形状校验由 `write_grok_provider_live` 的非官方分支负责。
pub fn write_grok_live_settings(settings: &Value) -> Result<(), AppError> {
    let config = settings
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.config.missing",
                "Grok Build 配置缺少 config 字段",
                "Grok Build configuration is missing the config field",
            )
        })?;
    validate_config_toml_syntax(config)?;
    let path = get_grok_config_path();
    backup_current_grok_config(&path, config)?;
    write_text_file(&path, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn valid_config() -> &'static str {
        r#"[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://example.com/v1"
name = "Example"
api_key = "secret"
api_backend = "responses"
context_window = 500000
"#
    }

    fn valid_env_key_config() -> &'static str {
        r#"[models]
default = "grok-env"

[model."grok-env"]
model = "grok-4.5"
base_url = "https://example.com/v1"
name = "Example Env"
env_key = "GROK_TEST_API_KEY"
api_backend = "responses"
context_window = 500000
"#
    }

    #[test]
    fn validates_expected_config_shape() {
        validate_config_toml(valid_config()).expect("valid Grok Build config");
        validate_config_toml(valid_env_key_config()).expect("valid env_key configuration");
    }

    #[test]
    fn syntax_validation_accepts_official_snapshots() {
        validate_config_toml_syntax("").expect("empty official snapshot");
        validate_config_toml_syntax("[mcp_servers.echo]\ncommand = \"echo\"\n")
            .expect("official-mode config without model tables");
        assert!(validate_config_toml_syntax("not = [valid").is_err());
    }

    #[test]
    fn official_live_config_detection() {
        // 官方态：完全没有自定义模型痕迹
        assert!(is_official_live_config(""));
        assert!(is_official_live_config("  \n# comment only\n"));
        assert!(is_official_live_config(
            "[mcp_servers.echo]\ncommand = \"echo\"\n"
        ));

        // 出现过任一自定义键（哪怕残缺）都不是官方态，交给强校验报错
        assert!(!is_official_live_config(valid_config()));
        assert!(!is_official_live_config("[models]\ndefault = \"x\"\n"));
        assert!(!is_official_live_config("[model.x]\nmodel = \"x\"\n"));

        // 语法不合法不是官方态
        assert!(!is_official_live_config("not = [valid"));
    }

    #[test]
    fn rejects_missing_selected_model_table() {
        let error = validate_config_toml("[models]\ndefault = \"grok-4.5\"\n")
            .expect_err("missing model table should fail");
        assert!(error.to_string().contains("model"));
    }

    #[test]
    fn rejects_config_without_api_key_or_env_key() {
        let config = valid_config().replace("api_key = \"secret\"\n", "");
        let error = validate_config_toml(&config).expect_err("credentials should be required");
        assert!(error.to_string().contains("api_key"));
        assert!(error.to_string().contains("env_key"));
    }

    #[test]
    fn extracts_selected_model_and_updates_takeover_fields() {
        let selected = extract_model_config(valid_config()).expect("selected model");
        assert_eq!(selected.profile, "grok-4.5");
        assert_eq!(selected.model, "grok-4.5");
        assert_eq!(selected.base_url, "https://example.com/v1");

        let updated = apply_proxy_takeover(
            valid_config(),
            "http://127.0.0.1:15721/grokbuild/v1",
            "PROXY_MANAGED",
        )
        .expect("takeover config");
        let selected = extract_model_config(&updated).expect("updated selected model");
        assert_eq!(selected.base_url, "http://127.0.0.1:15721/grokbuild/v1");
        assert_eq!(selected.api_key.as_deref(), Some("PROXY_MANAGED"));
        assert!(has_proxy_placeholder(&updated, "PROXY_MANAGED"));
    }

    #[test]
    fn takeover_preserves_env_key_profile_and_injects_inline_placeholder() {
        let direct_config = valid_env_key_config().replace(
            "api_backend = \"responses\"",
            "api_backend = \"chat_completions\"",
        );
        let updated = apply_proxy_takeover(
            &direct_config,
            "http://127.0.0.1:15721/grokbuild/v1",
            "PROXY_MANAGED",
        )
        .expect("takeover config");
        let selected = extract_model_config(&updated).expect("updated selected model");

        assert_eq!(selected.profile, "grok-env");
        assert_eq!(selected.env_key.as_deref(), Some("GROK_TEST_API_KEY"));
        assert_eq!(selected.api_key.as_deref(), Some("PROXY_MANAGED"));
        assert_eq!(selected.api_backend, DEFAULT_API_BACKEND);
    }

    #[test]
    fn takeover_updates_every_model_and_the_shared_endpoint() {
        let config = format!(
            "{}\n[model.worker]\nmodel = \"worker-model\"\nbase_url = \"https://worker.example/v1\"\nname = \"Worker\"\nenv_key = \"WORKER_KEY\"\napi_backend = \"chat_completions\"\ncontext_window = 128000\ncustom_flag = true\n",
            valid_config()
        );

        let updated = apply_proxy_takeover(
            &config,
            "http://127.0.0.1:15721/grokbuild/v1",
            "PROXY_MANAGED",
        )
        .expect("take over every model");
        let document = updated.parse::<toml::Value>().expect("updated TOML");
        let models = document["model"].as_table().expect("model table");
        assert_eq!(models.len(), 2);
        for model in models.values() {
            let model = model.as_table().expect("model profile");
            assert_eq!(
                model.get("base_url").and_then(toml::Value::as_str),
                Some("http://127.0.0.1:15721/grokbuild/v1")
            );
            assert_eq!(
                model.get("api_key").and_then(toml::Value::as_str),
                Some("PROXY_MANAGED")
            );
            assert_eq!(
                model.get("api_backend").and_then(toml::Value::as_str),
                Some(DEFAULT_API_BACKEND)
            );
        }
        assert_eq!(
            document["endpoints"]["models_base_url"].as_str(),
            Some("http://127.0.0.1:15721/grokbuild/v1")
        );
        assert_eq!(
            document["model"]["worker"]["custom_flag"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn privacy_and_takeover_reject_scalar_sections_without_panicking() {
        let privacy_error = apply_privacy_protection_config_text("features = false\n")
            .expect_err("scalar features section must be rejected");
        assert!(privacy_error.to_string().contains("features"));

        let config = format!("endpoints = \"legacy\"\n\n{}", valid_config());
        let takeover_error = apply_proxy_takeover(
            &config,
            "http://127.0.0.1:15721/grokbuild/v1",
            "PROXY_MANAGED",
        )
        .expect_err("scalar endpoints section must be rejected");
        assert!(takeover_error.to_string().contains("endpoints"));
    }

    #[test]
    fn emergency_cleanup_removes_takeover_fields_from_every_model_and_endpoint() {
        let config = format!(
            "{}\n[model.worker]\nmodel = \"worker-model\"\nbase_url = \"http://localhost:15721/grokbuild/v1\"\nname = \"Worker\"\napi_key = \"PROXY_MANAGED\"\napi_backend = \"responses\"\ncontext_window = 128000\n\n[endpoints]\nmodels_base_url = \"http://127.0.0.1:15721/grokbuild/v1\"\n",
            valid_config()
                .replace("https://example.com/v1", "http://127.0.0.1:15721/grokbuild/v1")
                .replace("api_key = \"secret\"", "api_key = \"PROXY_MANAGED\"")
        );

        let cleaned = remove_proxy_takeover_fields(&config, "PROXY_MANAGED", |url| {
            url.contains("127.0.0.1:15721") || url.contains("localhost:15721")
        })
        .expect("cleanup takeover fields");
        let document = cleaned.parse::<toml::Value>().expect("cleaned TOML");
        for model in document["model"].as_table().expect("model table").values() {
            let model = model.as_table().expect("model profile");
            assert!(!model.contains_key("api_key"));
            assert!(!model.contains_key("base_url"));
        }
        assert!(document.get("endpoints").is_none());
        assert!(!has_proxy_placeholder(&cleaned, "PROXY_MANAGED"));
    }

    #[test]
    fn emergency_cleanup_removes_owned_remote_url_but_keeps_other_models() {
        let config = r#"[models]
default = "owned"

[model.owned]
model = "owned"
base_url = "http://192.168.50.20:15721/grokbuild/v1"
name = "Owned"
api_key = "ccs-0123456789abcdef0123456789abcdef"
api_backend = "responses"
context_window = 128000

[model.user]
model = "user"
base_url = "https://user.example/v1"
name = "User"
api_key = "user-secret"
api_backend = "responses"
context_window = 128000

[endpoints]
models_base_url = "http://192.168.50.20:15721/grokbuild/v1"
"#;

        let cleaned =
            remove_proxy_takeover_fields_if(config, |value| value.starts_with("ccs-"), |_| false)
                .expect("clean remote takeover fields");
        let document = cleaned.parse::<toml::Value>().expect("cleaned TOML");
        let owned = document["model"]["owned"].as_table().expect("owned model");
        assert!(!owned.contains_key("api_key"));
        assert!(!owned.contains_key("base_url"));
        assert_eq!(
            document["model"]["user"]["api_key"].as_str(),
            Some("user-secret")
        );
        assert_eq!(
            document["model"]["user"]["base_url"].as_str(),
            Some("https://user.example/v1")
        );
        assert!(document.get("endpoints").is_none());
    }

    #[test]
    fn provider_profile_merge_preserves_live_global_and_mcp_settings() {
        let live = r#"[features]
telemetry = true

[harness]
custom_future_flag = "keep"

[mcp_servers.echo]
command = "echo"

[models]
default = "old"

[model.old]
model = "old"
base_url = "https://old.example/v1"
name = "Old"
api_key = "old-secret"
api_backend = "responses"
context_window = 1000
"#;

        let merged = merge_provider_profile_config_text(live, valid_config()).expect("merge");
        let document = merged.parse::<toml::Value>().expect("merged TOML");
        assert_eq!(document["features"]["telemetry"].as_bool(), Some(true));
        assert_eq!(
            document["harness"]["custom_future_flag"].as_str(),
            Some("keep")
        );
        assert_eq!(
            document["mcp_servers"]["echo"]["command"].as_str(),
            Some("echo")
        );
        assert!(document["model"].get("old").is_none());
        assert!(document["model"].get("grok-4.5").is_some());

        let profile = extract_provider_profile_config_text(&merged).expect("extract profile");
        assert!(!profile.contains("features"));
        assert!(!profile.contains("harness"));
        assert!(!profile.contains("mcp_servers"));
        validate_config_toml(&profile).expect("profile remains valid");

        let official = remove_provider_profile_config_text(&merged).expect("official config");
        assert!(is_official_live_config(&official));
        assert!(official.contains("custom_future_flag"));
        assert!(official.contains("mcp_servers.echo"));
    }

    #[test]
    #[serial]
    fn resolves_api_key_from_configured_environment_variable() {
        let original = std::env::var_os("GROK_TEST_API_KEY");
        std::env::set_var("GROK_TEST_API_KEY", "env-secret");

        let credentials = extract_credentials(valid_env_key_config()).expect("credentials");

        assert_eq!(credentials.0, "https://example.com/v1");
        assert_eq!(credentials.1, "env-secret");
        match original {
            Some(value) => std::env::set_var("GROK_TEST_API_KEY", value),
            None => std::env::remove_var("GROK_TEST_API_KEY"),
        }
    }

    /// 构造一个 `env_key` 指向未设置环境变量的 config——这是"声明了间接引用但
    /// 该变量不存在"的场景，修复前会静默兜底到 `XAI_API_KEY`。
    fn env_key_unset_config() -> &'static str {
        r#"[models]
default = "grok-env"

[model."grok-env"]
model = "grok-4.5"
base_url = "https://attacker.example/v1"
name = "Attacker Env"
env_key = "GROK_TEST_DEFINITELY_UNSET_VAR"
api_backend = "responses"
context_window = 500000
"#
    }

    #[test]
    #[serial]
    fn does_not_fall_back_to_xai_api_key_when_declared_env_key_is_unset() {
        // 即使进程里恰好设了 XAI_API_KEY，也不能被静默借用到别的 base_url 上。
        let original_xai = std::env::var_os("XAI_API_KEY");
        let original_unset = std::env::var_os("GROK_TEST_DEFINITELY_UNSET_VAR");
        std::env::set_var("XAI_API_KEY", "xai-secret-should-not-leak");
        std::env::remove_var("GROK_TEST_DEFINITELY_UNSET_VAR");

        let credentials = extract_credentials(env_key_unset_config());

        assert!(
            credentials.is_none(),
            "declared env_key unset must yield None, never a borrowed XAI_API_KEY; got {credentials:?}"
        );

        match original_xai {
            Some(value) => std::env::set_var("XAI_API_KEY", value),
            None => std::env::remove_var("XAI_API_KEY"),
        }
        match original_unset {
            Some(value) => std::env::set_var("GROK_TEST_DEFINITELY_UNSET_VAR", value),
            None => std::env::remove_var("GROK_TEST_DEFINITELY_UNSET_VAR"),
        }
    }

    #[test]
    fn strips_projected_mcp_servers_without_touching_model_config() {
        let mut settings = json!({
            "config": format!(
                "{}\n[mcp_servers.echo]\ncommand = \"echo\"\n",
                valid_config()
            )
        });

        strip_grok_mcp_servers_from_settings(&mut settings).expect("strip MCP servers");

        let config = settings.get("config").and_then(Value::as_str).unwrap();
        assert!(!config.contains("mcp_servers"));
        assert!(config.contains("model = \"grok-4.5\""));
        validate_config_toml(config).expect("stripped config remains valid");
    }

    #[test]
    #[serial]
    fn official_provider_roundtrips_without_custom_model_tables() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let original_config = std::env::var_os("GROK_CONFIG");
        let original_grok_home = std::env::var_os("GROK_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::remove_var("GROK_CONFIG");
        std::env::remove_var("GROK_HOME");

        let existing = format!(
            "[features]\ntelemetry = false\n\n[mcp_servers.echo]\ncommand = \"echo\"\n\n{}",
            valid_config()
        );
        write_grok_live_settings(&json!({ "config": existing })).expect("seed full live config");

        // 官方条目：仅清掉自定义模型字段，保留全局配置与 MCP 投影。
        let mut official = Provider::with_id(
            "grokbuild-official".to_string(),
            "Grok Official".to_string(),
            json!({ "config": "" }),
            None,
        );
        official.category = Some("official".to_string());
        write_grok_provider_live(&official).expect("official empty config is writable");
        let official_config =
            fs::read_to_string(get_grok_config_path()).expect("read official config");
        assert!(official_config.contains("telemetry = false"));
        assert!(official_config.contains("mcp_servers.echo"));
        assert!(is_official_live_config(&official_config));

        // 官方态 live（如 MCP 投影补写后）无自定义模型表，读取与原样写回都必须可用
        let official_live = "[mcp_servers.echo]\ncommand = \"echo\"\n";
        write_grok_live_settings(&json!({ "config": official_live }))
            .expect("official-mode live is writable for backup restore");
        let settings = read_grok_live_settings().expect("official-mode live is readable");
        assert_eq!(
            settings.get("config").and_then(Value::as_str),
            Some(official_live)
        );

        // 非官方供应商仍要求完整的自定义模型配置
        let custom = Provider::with_id(
            "custom".to_string(),
            "Custom".to_string(),
            json!({ "config": "" }),
            None,
        );
        assert!(write_grok_provider_live(&custom).is_err());

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match original_config {
            Some(value) => std::env::set_var("GROK_CONFIG", value),
            None => std::env::remove_var("GROK_CONFIG"),
        }
        match original_grok_home {
            Some(value) => std::env::set_var("GROK_HOME", value),
            None => std::env::remove_var("GROK_HOME"),
        }
    }

    #[test]
    #[serial]
    fn writes_and_reads_live_config() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let original_config = std::env::var_os("GROK_CONFIG");
        let original_grok_home = std::env::var_os("GROK_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::remove_var("GROK_CONFIG");
        std::env::remove_var("GROK_HOME");

        let provider = Provider::with_id(
            "grok".to_string(),
            "Example".to_string(),
            json!({ "config": valid_config() }),
            None,
        );
        write_grok_provider_live(&provider).expect("write live config");

        let path = get_grok_config_path();
        assert_eq!(path, temp.path().join(".grok").join("config.toml"));
        assert_eq!(
            fs::read_to_string(path).expect("read config"),
            valid_config()
        );
        assert_eq!(
            read_grok_live_settings()
                .expect("read live settings")
                .get("config")
                .and_then(Value::as_str),
            Some(valid_config())
        );

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match original_config {
            Some(value) => std::env::set_var("GROK_CONFIG", value),
            None => std::env::remove_var("GROK_CONFIG"),
        }
        match original_grok_home {
            Some(value) => std::env::set_var("GROK_HOME", value),
            None => std::env::remove_var("GROK_HOME"),
        }
    }

    #[test]
    #[serial]
    fn resolves_grok_config_environment_contract() {
        let temp = TempDir::new().expect("temp dir");
        let original_config = std::env::var_os("GROK_CONFIG");
        let original_home = std::env::var_os("GROK_HOME");

        std::env::set_var("GROK_HOME", temp.path().join("home"));
        std::env::remove_var("GROK_CONFIG");
        let home_location = get_grok_config_location();
        assert_eq!(home_location.source, "GROK_HOME");
        assert_eq!(
            PathBuf::from(home_location.path),
            temp.path().join("home").join("config.toml")
        );

        std::env::set_var("GROK_CONFIG", temp.path().join("explicit.toml"));
        let explicit_location = get_grok_config_location();
        assert_eq!(explicit_location.source, "GROK_CONFIG");
        assert_eq!(
            PathBuf::from(explicit_location.path),
            temp.path().join("explicit.toml")
        );

        match original_config {
            Some(value) => std::env::set_var("GROK_CONFIG", value),
            None => std::env::remove_var("GROK_CONFIG"),
        }
        match original_home {
            Some(value) => std::env::set_var("GROK_HOME", value),
            None => std::env::remove_var("GROK_HOME"),
        }
    }

    #[test]
    #[serial]
    fn backs_up_restores_and_deletes_global_config() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let original_config = std::env::var_os("GROK_CONFIG");
        let original_grok_home = std::env::var_os("GROK_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::remove_var("GROK_CONFIG");
        std::env::remove_var("GROK_HOME");

        let first = "[features]\ntelemetry = true\n";
        let second = "[features]\ntelemetry = false\n";
        write_grok_live_settings(&json!({ "config": first })).expect("first write");
        write_grok_live_settings(&json!({ "config": second })).expect("second write");

        let backups = list_grok_config_backups().expect("list backups");
        assert_eq!(backups.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir_mode = fs::metadata(get_grok_config_backup_dir())
                .expect("backup dir metadata")
                .permissions()
                .mode()
                & 0o777;
            let file_mode = fs::metadata(&backups[0].path)
                .expect("backup file metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dir_mode, 0o700);
            assert_eq!(file_mode, 0o600);
        }
        let filename = backups[0].filename.clone();
        assert_eq!(
            fs::read_to_string(&backups[0].path).expect("backup content"),
            first
        );

        let restored = restore_grok_config_backup(&filename).expect("restore backup");
        assert_eq!(restored, first);
        assert_eq!(
            fs::read_to_string(get_grok_config_path()).expect("restored live"),
            first
        );
        assert!(delete_grok_config_backup(&filename).expect("delete backup"));

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match original_config {
            Some(value) => std::env::set_var("GROK_CONFIG", value),
            None => std::env::remove_var("GROK_CONFIG"),
        }
        match original_grok_home {
            Some(value) => std::env::set_var("GROK_HOME", value),
            None => std::env::remove_var("GROK_HOME"),
        }
    }
}

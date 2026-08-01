//! MCP (Model Context Protocol) 服务器管理模块
//!
//! 本模块负责 MCP 服务器配置的验证、同步和导入导出。
//!
//! ## 模块结构
//!
//! - `validation` - 服务器配置验证
//! - `claude` - Claude MCP 同步和导入
//! - `codex` - Codex MCP 同步和导入（含 TOML 转换）
//! - `gemini` - Gemini MCP 同步和导入
//! - `opencode` - OpenCode MCP 同步和导入（含 local/remote 格式转换）
//! - `hermes` - Hermes MCP 同步和导入

mod claude;
mod codex;
mod gemini;
mod grokbuild;
mod hermes;
mod opencode;
mod validation;

use serde_json::Value;

/// Remove only the portion of `original` that survived a lossy live projection. Object and array
/// remnants are retained so a later live edit cannot erase values that the target dialect cannot
/// represent.
fn subtract_projected_value(original: &Value, projected: &Value) -> Option<Value> {
    match (original, projected) {
        (Value::Object(original), Value::Object(projected)) => {
            let mut preserved = original.clone();
            for (key, projected_value) in projected {
                let Some(original_value) = preserved.remove(key) else {
                    continue;
                };
                if let Some(remnant) = subtract_projected_value(&original_value, projected_value) {
                    preserved.insert(key.clone(), remnant);
                }
            }
            (!preserved.is_empty()).then_some(Value::Object(preserved))
        }
        (Value::Array(original), Value::Array(projected)) => {
            // Core arrays such as `args` may project only string members. Match projected values
            // one-for-one and retain every unmatched (therefore unprojectable) DB element.
            let mut unmatched_projected = projected.clone();
            let mut preserved = Vec::new();
            for value in original {
                if let Some(index) = unmatched_projected.iter().position(|item| item == value) {
                    unmatched_projected.remove(index);
                } else {
                    preserved.push(value.clone());
                }
            }
            (!preserved.is_empty()).then_some(Value::Array(preserved))
        }
        // A scalar (or a shape changed by normalization) survived projection as one unit.
        _ => None,
    }
}

fn overlay_live_value(preserved: Option<Value>, live: &Value) -> Value {
    match (preserved, live) {
        (Some(Value::Object(mut preserved)), Value::Object(live)) => {
            for (key, live_value) in live {
                let remnant = preserved.remove(key);
                preserved.insert(key.clone(), overlay_live_value(remnant, live_value));
            }
            Value::Object(preserved)
        }
        (Some(Value::Array(mut preserved)), Value::Array(live)) => {
            let mut merged = live.clone();
            merged.append(&mut preserved);
            Value::Array(merged)
        }
        (_, live) => live.clone(),
    }
}

/// Merge a dialect-derived live spec into the shared DB representation while retaining only DB
/// remnants that the dialect's canonical projection could not express.
fn merge_lossy_live_spec(
    db_spec: &Value,
    live_spec: &Value,
    projected_db_spec: Result<Value, crate::error::AppError>,
    stale_aliases: &[&str],
) -> Value {
    let preserved = match projected_db_spec {
        Ok(projected) => subtract_projected_value(db_spec, &projected),
        Err(_) => Some(db_spec.clone()),
    };
    let mut merged = overlay_live_value(preserved, live_spec);
    if let Some(object) = merged.as_object_mut() {
        for alias in stale_aliases {
            object.remove(*alias);
        }
    }
    merged
}

// 重新导出公共 API
pub use claude::{
    import_from_claude, remove_server_from_claude, sync_enabled_to_claude,
    sync_single_server_to_claude,
};
pub(crate) use codex::merge_codex_live_spec;
pub use codex::{
    codex_specs_equivalent, collect_live_codex_server_specs, import_from_codex,
    remove_server_from_codex, sync_enabled_to_codex, sync_single_server_to_codex,
};
pub use gemini::{
    import_from_gemini, remove_server_from_gemini, sync_enabled_to_gemini,
    sync_single_server_to_gemini,
};
pub(crate) use grokbuild::merge_grokbuild_live_spec;
pub use grokbuild::{
    collect_live_grokbuild_server_specs, grokbuild_specs_equivalent, import_from_grokbuild,
    remove_server_from_grokbuild, sync_single_server_to_grokbuild,
};
pub use hermes::{import_from_hermes, remove_server_from_hermes, sync_single_server_to_hermes};
pub use opencode::{
    import_from_opencode, remove_server_from_opencode, sync_single_server_to_opencode,
};

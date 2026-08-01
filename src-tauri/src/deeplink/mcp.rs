//! MCP server import from deep link
//!
//! Handles batch import of MCP server configurations via ccswitch:// URLs.

use super::utils::decode_base64_param;
use super::DeepLinkImportRequest;
use crate::app_config::{McpApps, McpServer};
use crate::error::AppError;
use crate::services::McpService;
use crate::store::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP import result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportResult {
    /// Number of successfully imported MCP servers
    pub imported_count: usize,
    /// IDs of successfully imported MCP servers
    pub imported_ids: Vec<String>,
    /// Failed imports with error messages
    pub failed: Vec<McpImportError>,
}

/// MCP import error
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportError {
    /// MCP server ID
    pub id: String,
    /// Error message
    pub error: String,
}

/// Import MCP servers from deep link request
///
/// This function handles batch import of MCP servers from standard MCP JSON format.
/// If a server already exists, only the apps flags are merged (existing config preserved).
pub fn import_mcp_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<McpImportResult, AppError> {
    // Verify this is an MCP request
    if request.resource != "mcp" {
        return Err(AppError::InvalidInput(format!(
            "Expected mcp resource, got '{}'",
            request.resource
        )));
    }

    // Extract and validate apps parameter
    let apps_str = request
        .apps
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'apps' parameter for MCP".to_string()))?;

    // Parse apps into McpApps struct
    let target_apps = parse_mcp_apps(apps_str)?;
    // DeepLinkImportRequest documents omitted `enabled` as false. Only an explicit true may
    // write executable MCP configuration into the selected applications.
    let enabled = request.enabled.unwrap_or(false);

    // Extract config
    let config_b64 = request
        .config
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'config' parameter for MCP".to_string()))?;

    // Decode Base64 config
    let decoded = decode_base64_param("config", config_b64)?;

    let config_str = String::from_utf8(decoded)
        .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in config: {e}")))?;

    // Parse JSON
    let config_json: Value = serde_json::from_str(&config_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid JSON in MCP config: {e}")))?;

    // Extract mcpServers object
    let mcp_servers = config_json
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            AppError::InvalidInput("MCP config must contain 'mcpServers' object".to_string())
        })?;

    if mcp_servers.is_empty() {
        return Err(AppError::InvalidInput(
            "No MCP servers found in config".to_string(),
        ));
    }

    // Get existing servers to check for duplicates
    let existing_servers = state.db.get_all_mcp_servers()?;

    // Import each MCP server
    let mut imported_ids = Vec::new();
    let mut failed = Vec::new();

    for (id, server_spec) in mcp_servers.iter() {
        // Check if server already exists
        let server = if let Some(existing) = existing_servers.get(id) {
            // Server exists - update only the apps named by this link, preserving every
            // unspecified app and the existing server configuration.
            log::info!("MCP server '{id}' already exists, updating selected apps only");

            let merged_apps = apply_mcp_apps(&existing.apps, &target_apps, enabled);

            McpServer {
                id: existing.id.clone(),
                name: existing.name.clone(),
                server: existing.server.clone(), // Keep existing server config
                apps: merged_apps,               // Merged apps
                description: existing.description.clone(),
                homepage: existing.homepage.clone(),
                docs: existing.docs.clone(),
                tags: existing.tags.clone(),
            }
        } else {
            // New server - create with provided config
            log::info!("Creating new MCP server: {id}");
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: server_spec.clone(),
                apps: if enabled {
                    target_apps.clone()
                } else {
                    McpApps::default()
                },
                description: None,
                homepage: None,
                docs: None,
                tags: vec!["imported".to_string()],
            }
        };

        match McpService::upsert_server(state, server) {
            Ok(_) => {
                imported_ids.push(id.clone());
                log::info!("Successfully imported/updated MCP server: {id}");
            }
            Err(e) => {
                failed.push(McpImportError {
                    id: id.clone(),
                    error: format!("{e}"),
                });
                log::warn!("Failed to import MCP server '{id}': {e}");
            }
        }
    }

    Ok(McpImportResult {
        imported_count: imported_ids.len(),
        imported_ids,
        failed,
    })
}

/// Parse apps string into McpApps struct
pub(crate) fn parse_mcp_apps(apps_str: &str) -> Result<McpApps, AppError> {
    let mut apps = McpApps {
        claude: false,
        codex: false,
        gemini: false,
        grokbuild: false,
        opencode: false,
        hermes: false,
    };

    for app in apps_str.split(',') {
        match app.trim() {
            "claude" => apps.claude = true,
            "codex" => apps.codex = true,
            "gemini" => apps.gemini = true,
            "grokbuild" | "grok" => apps.grokbuild = true,
            "opencode" => apps.opencode = true,
            "openclaw" => {
                // OpenClaw doesn't support MCP, ignore silently
                log::debug!("OpenClaw doesn't support MCP, ignoring in apps parameter");
            }
            "hermes" => apps.hermes = true,
            other => {
                return Err(AppError::InvalidInput(format!(
                    "Invalid app in 'apps': {other}"
                )))
            }
        }
    }

    if apps.is_empty() {
        return Err(AppError::InvalidInput(
            "At least one app must be specified in 'apps'".to_string(),
        ));
    }

    Ok(apps)
}

fn apply_mcp_apps(existing: &McpApps, target: &McpApps, enabled: bool) -> McpApps {
    let mut merged = existing.clone();
    for app in target.enabled_apps() {
        merged.set_enabled_for(&app, enabled);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::prelude::*;
    use serial_test::serial;
    use std::sync::Arc;

    fn import_request(id: &str, apps: &str, enabled: Option<bool>) -> DeepLinkImportRequest {
        let config = BASE64_STANDARD.encode(
            serde_json::json!({
                "mcpServers": {
                    (id): { "command": "echo", "args": [id] }
                }
            })
            .to_string(),
        );
        DeepLinkImportRequest {
            resource: "mcp".to_string(),
            apps: Some(apps.to_string()),
            config: Some(config),
            enabled,
            ..Default::default()
        }
    }

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

    #[test]
    fn enabled_apps_merge_covers_every_supported_mcp_client() {
        let existing = McpApps {
            claude: true,
            ..McpApps::default()
        };
        let target = McpApps {
            codex: true,
            gemini: true,
            grokbuild: true,
            opencode: true,
            hermes: true,
            ..McpApps::default()
        };
        let merged = apply_mcp_apps(&existing, &target, true);

        assert!(merged.claude);
        assert!(merged.codex);
        assert!(merged.gemini);
        assert!(merged.grokbuild);
        assert!(merged.opencode);
        assert!(merged.hermes);
    }

    #[test]
    fn disabled_import_only_disables_apps_named_by_the_link() {
        let existing = McpApps {
            claude: true,
            codex: true,
            gemini: true,
            ..McpApps::default()
        };
        let target = McpApps {
            codex: true,
            gemini: true,
            ..McpApps::default()
        };

        let merged = apply_mcp_apps(&existing, &target, false);
        assert!(merged.claude, "unspecified apps must be preserved");
        assert!(!merged.codex);
        assert!(!merged.gemini);
    }

    #[test]
    fn enabled_defaults_to_false() {
        let omitted = DeepLinkImportRequest::default();
        let explicit_false = DeepLinkImportRequest {
            enabled: Some(false),
            ..Default::default()
        };
        let explicit_true = DeepLinkImportRequest {
            enabled: Some(true),
            ..Default::default()
        };

        assert!(!omitted.enabled.unwrap_or(false));
        assert!(!explicit_false.enabled.unwrap_or(false));
        assert!(explicit_true.enabled.unwrap_or(false));
    }

    #[test]
    #[serial]
    fn import_enabled_semantics_persist_disabled_and_only_change_selected_apps() {
        let home = TestHome::new();
        std::fs::create_dir_all(home.dir.path().join(".codex")).unwrap();
        let db = Arc::new(crate::database::Database::memory().expect("memory db"));
        let state = AppState::new(db.clone());

        import_mcp_from_deeplink(&state, import_request("omitted", "codex", None))
            .expect("omitted enabled import");
        import_mcp_from_deeplink(
            &state,
            import_request("explicit-false", "codex", Some(false)),
        )
        .expect("explicit false import");

        let servers = db.get_all_mcp_servers().unwrap();
        assert!(!servers["omitted"].apps.codex);
        assert!(!servers["explicit-false"].apps.codex);
        let codex_config = home.dir.path().join(".codex").join("config.toml");
        assert!(
            !codex_config.exists(),
            "disabled imports must not project executable live configuration"
        );

        import_mcp_from_deeplink(
            &state,
            import_request("selected", "claude,codex", Some(true)),
        )
        .expect("explicit true import");
        let servers = db.get_all_mcp_servers().unwrap();
        assert!(servers["selected"].apps.claude);
        assert!(servers["selected"].apps.codex);
        assert!(
            std::fs::read_to_string(&codex_config)
                .unwrap()
                .contains("selected"),
            "explicit true must project to selected apps"
        );

        import_mcp_from_deeplink(&state, import_request("selected", "codex", Some(false)))
            .expect("disable selected app");
        let servers = db.get_all_mcp_servers().unwrap();
        assert!(
            servers["selected"].apps.claude,
            "apps omitted by the link must retain their previous state"
        );
        assert!(!servers["selected"].apps.codex);
        assert!(!std::fs::read_to_string(&codex_config)
            .unwrap()
            .contains("selected"));
    }
}

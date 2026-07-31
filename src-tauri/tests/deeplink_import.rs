use std::sync::Arc;

use cc_switch_lib::{
    deeplink_error_payload, import_provider_from_deeplink, parse_deeplink_url, redact_url_for_log,
    AppState, Database,
};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn deeplink_import_claude_provider_persists_to_db() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let url = "ccswitch://v1/import?resource=provider&app=claude&name=DeepLink%20Claude&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-test-claude-key&model=claude-sonnet-4&icon=claude";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import provider from deeplink");

    // Verify DB state
    let providers = db.get_all_providers("claude").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, request.name.clone().unwrap());
    assert_eq!(provider.website_url.as_deref(), request.homepage.as_deref());
    assert_eq!(provider.icon.as_deref(), Some("claude"));
    let auth_token = provider
        .settings_config
        .pointer("/env/ANTHROPIC_AUTH_TOKEN")
        .and_then(|v| v.as_str());
    let base_url = provider
        .settings_config
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str());
    assert_eq!(auth_token, request.api_key.as_deref());
    assert_eq!(base_url, request.endpoint.as_deref());
}

#[test]
fn deeplink_import_codex_provider_builds_auth_and_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let url = "ccswitch://v1/import?resource=provider&app=codex&name=DeepLink%20Codex&homepage=https%3A%2F%2Fopenai.example&endpoint=https%3A%2F%2Fapi.openai.example%2Fv1&apiKey=sk-test-codex-key&model=gpt-4o&icon=openai";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import provider from deeplink");

    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, request.name.clone().unwrap());
    assert_eq!(provider.website_url.as_deref(), request.homepage.as_deref());
    assert_eq!(provider.icon.as_deref(), Some("openai"));
    let auth_value = provider
        .settings_config
        .pointer("/auth/OPENAI_API_KEY")
        .and_then(|v| v.as_str());
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(auth_value, request.api_key.as_deref());
    assert!(
        config_text.contains(request.endpoint.as_deref().unwrap()),
        "config.toml content should contain endpoint"
    );
    assert!(
        config_text.contains("model = \"gpt-4o\""),
        "config.toml content should contain model setting"
    );
}

#[test]
fn deeplink_redaction_never_exposes_secrets() {
    // Query values carry secrets: apiKey / usageApiKey / usageAccessToken /
    // base64 config blob ("Y2ZnLXNlbnRpbmVs" == "cfg-sentinel").
    let url = "ccswitch://v1/import?resource=provider&app=claude&apiKey=sk-sentinel-123&usageApiKey=uk-sentinel-456&usageAccessToken=uat-sentinel-789&config=Y2ZnLXNlbnRpbmVs";

    let redacted = redact_url_for_log(url);

    // Secret values must never appear in the redacted form.
    assert!(
        !redacted.contains("sk-sentinel-123"),
        "redacted: {redacted}"
    );
    assert!(
        !redacted.contains("uk-sentinel-456"),
        "redacted: {redacted}"
    );
    assert!(
        !redacted.contains("uat-sentinel-789"),
        "redacted: {redacted}"
    );
    assert!(
        !redacted.contains("Y2ZnLXNlbnRpbmVs"),
        "redacted: {redacted}"
    );

    // Scheme/host/path and the sorted query-key list stay for debugging.
    assert!(
        redacted.starts_with("ccswitch://v1/import"),
        "redacted: {redacted}"
    );
    assert!(redacted.contains("?[keys:"), "redacted: {redacted}");
    assert!(redacted.contains("apiKey"), "redacted: {redacted}");
    assert!(
        redacted.contains("usageAccessToken"),
        "redacted: {redacted}"
    );
}

#[test]
fn deeplink_error_payload_carries_redacted_url() {
    // Parseable URL -> Ok branch of the redactor ("?[keys:...]" form).
    let url = "ccswitch://v1/import?resource=bogus&apiKey=sk-sentinel-123";
    let payload = deeplink_error_payload(url, "unsupported resource");
    let payload_url = payload["url"].as_str().expect("url field");
    assert!(
        !payload_url.contains("sk-sentinel-123"),
        "payload url: {payload_url}"
    );
    assert!(
        payload_url.contains("?[keys:"),
        "payload url: {payload_url}"
    );
    assert_eq!(payload["error"].as_str(), Some("unsupported resource"));

    // Unparseable URL (no scheme) -> Err branch ("?[redacted]" form).
    let bad_url = "no scheme here?apiKey=sk-sentinel-123";
    let payload = deeplink_error_payload(bad_url, "invalid url");
    let payload_url = payload["url"].as_str().expect("url field");
    assert!(
        !payload_url.contains("sk-sentinel-123"),
        "payload url: {payload_url}"
    );
    assert_eq!(payload_url, "no scheme here?[redacted]");
    assert_eq!(payload["error"].as_str(), Some("invalid url"));
}

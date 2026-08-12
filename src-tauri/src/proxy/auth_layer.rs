//! Authentication for the local credential proxy.
//!
//! The proxy is intentionally zero-configuration for clients connecting over
//! loopback.  A listener bound to a non-loopback address, however, can receive
//! requests from other machines.  Those requests must present the gateway
//! token that is persisted alongside the Claude Desktop gateway configuration.

use crate::app_config::AppType;
use axum::{
    body::Body,
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use serde_json::Value;
use std::net::{IpAddr, SocketAddr};
use subtle::ConstantTimeEq;

use super::server::ProxyState;

/// Keep the existing CLI experience for local clients.  Claude Desktop keeps
/// its endpoint-specific check as an additional defence in depth, so changing
/// this policy does not remove that endpoint's loopback authentication.
const REQUIRE_TOKEN_ON_LOOPBACK: bool = false;

/// Require the gateway token for requests received from a non-loopback peer.
///
/// The accept loop injects the peer [`SocketAddr`] into request extensions. A
/// missing peer address is treated as remote (fail closed) rather than as
/// loopback, so a future transport change cannot accidentally disable auth.
pub(crate) async fn require_gateway_token(
    State(state): State<ProxyState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let peer = request.extensions().get::<SocketAddr>().copied();
    let is_loopback = peer.is_some_and(is_loopback_peer);

    if is_loopback && !REQUIRE_TOKEN_ON_LOOPBACK {
        return Ok(next.run(request).await);
    }

    let expected = state
        .db
        .spawn(crate::claude_desktop_config::get_or_create_gateway_token)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let presented = extract_bearer_token(request.headers())
        .unwrap_or("")
        .to_owned();

    if !constant_time_token_eq(&presented, &expected)
        && !accept_codex_native_oauth(&state, request.uri().path(), &presented).await
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}

/// The built-in Codex provider deliberately keeps ChatGPT OAuth in
/// `auth.json` while its `base_url` is projected to this gateway.  Codex has
/// no separate header slot for a gateway credential, so accepting that exact
/// local access token is the only way to keep an explicitly non-loopback
/// listener usable without replacing the upstream credential.  Keep this
/// exception narrow: only the official provider, only native Responses
/// endpoints, and never while automatic failover can route to another
/// provider. Every other remote request still requires the gateway token.
async fn accept_codex_native_oauth(
    state: &ProxyState,
    request_path: &str,
    presented: &str,
) -> bool {
    if !is_codex_native_auth_route(request_path) || presented.is_empty() {
        return false;
    }

    let presented = presented.to_owned();
    state
        .db
        .spawn(move |db| {
            let proxy_config = match futures::executor::block_on(
                db.get_proxy_config_for_app(AppType::Codex.as_str()),
            ) {
                Ok(config) => config,
                Err(_) => return Ok(false),
            };
            if proxy_config.auto_failover_enabled {
                return Ok(false);
            }

            let provider_id = crate::settings::get_effective_current_provider(db, &AppType::Codex)
                .ok()
                .flatten();
            if provider_id.as_deref() != Some(crate::database::CODEX_OFFICIAL_PROVIDER_ID) {
                return Ok(false);
            }

            let Some(provider) = db
                .get_provider_by_id(crate::database::CODEX_OFFICIAL_PROVIDER_ID, "codex")
                .ok()
                .flatten()
            else {
                return Ok(false);
            };
            if !crate::proxy::providers::is_codex_official_provider(&provider) {
                return Ok(false);
            }

            let auth_path = crate::codex_config::get_codex_auth_path();
            let auth: Value = match crate::config::read_json_file(&auth_path) {
                Ok(auth) => auth,
                Err(_) => return Ok(false),
            };
            Ok(codex_native_access_token(&auth)
                .is_some_and(|token| constant_time_token_eq(&presented, token)))
        })
        .await
        .unwrap_or(false)
}

fn is_codex_native_auth_route(path: &str) -> bool {
    matches!(
        path,
        "/responses"
            | "/v1/responses"
            | "/v1/v1/responses"
            | "/codex/v1/responses"
            | "/responses/compact"
            | "/v1/responses/compact"
            | "/v1/v1/responses/compact"
            | "/codex/v1/responses/compact"
    )
}

fn codex_native_access_token(auth: &Value) -> Option<&str> {
    auth.pointer("/tokens/access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

/// Extract a Bearer token without allocating or logging the credential.
pub(crate) fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let mut fields = value.splitn(2, |character: char| character.is_ascii_whitespace());
    let scheme = fields.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }

    let token = fields.next()?.trim();
    (!token.is_empty()).then_some(token)
}

/// Compare gateway tokens in constant time.
pub(crate) fn constant_time_token_eq(presented: &str, expected: &str) -> bool {
    presented.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() == 1
}

/// Return whether a peer address belongs to the loopback interface.
pub(crate) fn is_loopback_peer(peer: SocketAddr) -> bool {
    is_loopback_ip(peer.ip())
}

/// Return whether an address exposes the listener beyond loopback.
///
/// `localhost` is accepted by the UI and resolves to loopback at bind time;
/// treat it as loopback here as well. Invalid addresses are conservatively
/// considered exposed (the server will reject them before binding).
pub(crate) fn is_non_loopback_bind_address(address: &str) -> bool {
    let address = address.trim();
    if address.eq_ignore_ascii_case("localhost") {
        return false;
    }

    match address.parse::<IpAddr>() {
        Ok(ip) => !is_loopback_ip(ip),
        Err(_) => true,
    }
}

fn is_loopback_ip(ip: IpAddr) -> bool {
    if ip.is_loopback() {
        return true;
    }

    // Some platforms expose IPv4 peers as IPv4-mapped IPv6 addresses.
    match ip {
        IpAddr::V6(ipv6) => ipv6.to_ipv4().is_some_and(|ipv4| ipv4.is_loopback()),
        IpAddr::V4(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::HeaderValue;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[test]
    fn loopback_peers_are_recognized() {
        assert!(is_loopback_peer("127.0.0.1:15721".parse().unwrap()));
        assert!(is_loopback_peer("[::1]:15721".parse().unwrap()));
        assert!(is_loopback_peer(
            "[::ffff:127.0.0.1]:15721".parse().unwrap()
        ));
        assert!(!is_loopback_peer("192.168.1.20:15721".parse().unwrap()));
    }

    #[test]
    fn bind_exposure_detection_is_conservative() {
        assert!(!is_non_loopback_bind_address("localhost"));
        assert!(!is_non_loopback_bind_address("127.0.0.2"));
        assert!(!is_non_loopback_bind_address("::1"));
        assert!(is_non_loopback_bind_address("0.0.0.0"));
        assert!(is_non_loopback_bind_address("::"));
        assert!(is_non_loopback_bind_address("192.168.1.20"));
        assert!(is_non_loopback_bind_address("not-an-address"));
    }

    #[test]
    fn bearer_parser_is_case_insensitive_and_does_not_allocate() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("bEaReR ccs-token"));
        assert_eq!(extract_bearer_token(&headers), Some("ccs-token"));

        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic ccs-token"));
        assert_eq!(extract_bearer_token(&headers), None);
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(extract_bearer_token(&headers), None);
    }

    #[test]
    fn token_comparison_is_exact() {
        assert!(constant_time_token_eq("ccs-token", "ccs-token"));
        assert!(!constant_time_token_eq("ccs-toke", "ccs-token"));
        assert!(!constant_time_token_eq("ccs-tokenx", "ccs-token"));
    }

    #[test]
    fn native_codex_exception_is_limited_to_responses_routes() {
        assert!(is_codex_native_auth_route("/v1/responses"));
        assert!(is_codex_native_auth_route("/codex/v1/responses/compact"));
        assert!(!is_codex_native_auth_route("/v1/chat/completions"));
        assert!(!is_codex_native_auth_route("/grokbuild/v1/responses"));
    }

    #[test]
    fn native_codex_token_reads_only_nested_access_token() {
        let auth = serde_json::json!({
            "tokens": { "access_token": " oauth-access " },
            "access_token": "wrong"
        });
        assert_eq!(codex_native_access_token(&auth), Some("oauth-access"));
        assert_eq!(codex_native_access_token(&serde_json::json!({})), None);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn native_codex_exception_requires_current_official_provider_and_local_token() {
        let _settings = crate::settings::test_support::AmbientSettings::pin("{}");
        let db = Arc::new(crate::database::Database::memory().expect("init test db"));
        db.init_default_official_providers()
            .expect("seed official providers");
        db.set_current_provider(
            AppType::Codex.as_str(),
            crate::database::CODEX_OFFICIAL_PROVIDER_ID,
        )
        .expect("select official Codex provider");
        crate::settings::set_current_provider(
            &AppType::Codex,
            Some(crate::database::CODEX_OFFICIAL_PROVIDER_ID),
        )
        .expect("set local official Codex provider");
        crate::config::write_json_file(
            &crate::codex_config::get_codex_auth_path(),
            &serde_json::json!({
                "auth_mode": "chatgpt",
                "tokens": { "access_token": "oauth-access-token" }
            }),
        )
        .expect("write test Codex auth");

        let state = ProxyState {
            db: db.clone(),
            config: Arc::new(RwLock::new(crate::proxy::ProxyConfig::default())),
            status: Arc::new(RwLock::new(crate::proxy::ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            provider_router: Arc::new(crate::proxy::ProviderRouter::new(db.clone())),
            gemini_shadow: Arc::new(
                crate::proxy::providers::gemini_shadow::GeminiShadowStore::default(),
            ),
            codex_chat_history: Arc::new(
                crate::proxy::providers::codex_chat_history::CodexChatHistoryStore::default(),
            ),
            app_handle: None,
            failover_manager: Arc::new(crate::proxy::failover_switch::FailoverSwitchManager::new(
                db,
            )),
        };

        assert!(accept_codex_native_oauth(&state, "/v1/responses", "oauth-access-token").await);
        assert!(!accept_codex_native_oauth(&state, "/v1/responses", "wrong-token").await);
        assert!(
            !accept_codex_native_oauth(&state, "/grokbuild/v1/responses", "oauth-access-token")
                .await
        );

        let mut proxy_config = state
            .db
            .get_proxy_config_for_app(AppType::Codex.as_str())
            .await
            .expect("read Codex proxy config");
        proxy_config.auto_failover_enabled = true;
        state
            .db
            .update_proxy_config_for_app(proxy_config)
            .await
            .expect("enable failover");
        assert!(!accept_codex_native_oauth(&state, "/v1/responses", "oauth-access-token").await);
    }
}

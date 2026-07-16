//! Provider protocol-capability resolution.
//!
//! Centralizes "what can this provider's upstream actually do" so that status
//! reporting and continuation gating stop re-deriving it from scattered
//! name/format predicates. This module deliberately mirrors the
//! `model_capabilities.rs` shape:
//!
//! - an explicit declaration wins over any heuristic;
//! - when nothing is declared, fall back to the SAME format predicates that
//!   already drive the routing hot path (so a capability snapshot never
//!   disagrees with the transform actually selected);
//! - absence is `Unknown` (fail-open), never a silent `false`.
//!
//! It does NOT replace the hot-path predicates in `codex.rs` / `claude.rs`;
//! it composes them into a read-only snapshot for the status UI (D1) and to
//! express continuation eligibility (D3/D4), so working routing is untouched.

use super::codex::{
    codex_provider_uses_anthropic, codex_provider_uses_chat_completions, is_codex_official_provider,
};
use super::{claude_api_format_needs_transform, get_claude_api_format};
use crate::app_config::AppType;
use crate::provider::Provider;
use serde::{Deserialize, Serialize};

/// Confidence behind a resolved capability. Ordered weakest → strongest so the
/// resolver (and any future observed-capability cache) can keep the highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityConfidence {
    /// Inferred from provider/model name, base_url suffix, or wire_api guess.
    Heuristic,
    /// Explicit user/preset declaration (`meta.capabilities` / `apiFormat` / `wire_api`).
    Declared,
    /// A live capability probe confirmed it (e.g. the Copilot `/models` fetch).
    Probed,
    /// Observed from a real successful round-trip.
    Confirmed,
}

/// Effective support state for a single capability.
///
/// `Unknown` is intentionally distinct from `Unsupported`: it means "we have no
/// evidence either way" and callers must fail open, exactly like
/// [`crate::model_capabilities::ImageInputCapability`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CapabilityState {
    Supported { confidence: CapabilityConfidence },
    Unsupported { confidence: CapabilityConfidence },
    Unknown,
}

impl CapabilityState {
    fn from_declared(declared: Option<bool>) -> Option<Self> {
        match declared {
            Some(true) => Some(CapabilityState::Supported {
                confidence: CapabilityConfidence::Declared,
            }),
            Some(false) => Some(CapabilityState::Unsupported {
                confidence: CapabilityConfidence::Declared,
            }),
            None => None,
        }
    }

    fn heuristic(supported: bool) -> Self {
        if supported {
            CapabilityState::Supported {
                confidence: CapabilityConfidence::Heuristic,
            }
        } else {
            CapabilityState::Unsupported {
                confidence: CapabilityConfidence::Heuristic,
            }
        }
    }

    /// Resolve declared-first, else the given heuristic value.
    fn resolve(declared: Option<bool>, heuristic: Option<bool>) -> Self {
        if let Some(state) = Self::from_declared(declared) {
            return state;
        }
        match heuristic {
            Some(value) => Self::heuristic(value),
            None => CapabilityState::Unknown,
        }
    }

    #[cfg(test)]
    pub fn is_supported(&self) -> bool {
        matches!(self, CapabilityState::Supported { .. })
    }
}

/// Continuation is richer than a bool: a Chat/Anthropic upstream can only
/// emulate continuation by resending the full conversation (a strict downgrade
/// from server-side `previous_response_id`). We surface that as `Degraded`
/// rather than pretending equivalence, per the task's explicit requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationSupport {
    /// Native `/v1/responses` passthrough: server-side continuation is possible
    /// (and CC Switch's CodexCont folding is eligible on this chain).
    Native,
    /// Chat / Anthropic bridge: only client-side full-context replay. NOT
    /// equivalent to native Responses continuation.
    Degraded,
    /// Explicitly declared unsupported.
    Unsupported,
    Unknown,
}

impl ContinuationSupport {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContinuationSupport::Native => "native",
            ContinuationSupport::Degraded => "degraded",
            ContinuationSupport::Unsupported => "unsupported",
            ContinuationSupport::Unknown => "unknown",
        }
    }
}

/// The wire protocol a Codex/Claude request is actually forwarded as, after
/// all format resolution. Used for the D1 status display and to pick the
/// continuation flavor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireProtocol {
    Responses,
    ChatCompletions,
    Anthropic,
    /// Native Anthropic Messages (Claude app default) or Gemini native — not a
    /// Responses/Chat bridge; recorded for completeness.
    Native,
}

impl WireProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            WireProtocol::Responses => "responses",
            WireProtocol::ChatCompletions => "chat_completions",
            WireProtocol::Anthropic => "anthropic",
            WireProtocol::Native => "native",
        }
    }
}

/// Resolved protocol-capability snapshot for one provider on one endpoint.
///
/// This is the read model surfaced to the status UI and consulted for
/// continuation eligibility. It never fabricates support: undeclared,
/// non-inferable capabilities stay `Unknown`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilitySnapshot {
    /// The protocol the request is forwarded as (post format resolution).
    pub protocol: WireProtocol,
    pub supports_responses: CapabilityState,
    pub supports_chat_completions: CapabilityState,
    pub supports_streaming: CapabilityState,
    pub supports_tools: CapabilityState,
    pub supports_reasoning: CapabilityState,
    pub supports_previous_response_id: CapabilityState,
    pub continuation: ContinuationSupport,
    pub supports_response_item_ids: CapabilityState,
    pub supports_encrypted_reasoning: CapabilityState,
    pub supports_usage_metadata: CapabilityState,
}

/// Resolve the protocol a Codex provider's request is forwarded as.
fn codex_wire_protocol(provider: &Provider) -> WireProtocol {
    if codex_provider_uses_anthropic(provider) {
        WireProtocol::Anthropic
    } else if codex_provider_uses_chat_completions(provider) {
        WireProtocol::ChatCompletions
    } else {
        // The Codex client always speaks Responses to CC Switch; when neither a
        // Chat nor an Anthropic bridge is selected the upstream is native
        // Responses passthrough. Official Codex is also a Responses upstream.
        let _ = is_codex_official_provider(provider);
        WireProtocol::Responses
    }
}

/// Resolve the protocol a Claude provider's request is forwarded as.
fn claude_wire_protocol(provider: &Provider) -> WireProtocol {
    match get_claude_api_format(provider) {
        "openai_responses" => WireProtocol::Responses,
        "openai_chat" => WireProtocol::ChatCompletions,
        "gemini_native" => WireProtocol::Native,
        _ => WireProtocol::Native, // anthropic native
    }
}

/// Compute the wire protocol for any app type + provider.
pub fn resolve_wire_protocol(app_type: &AppType, provider: &Provider) -> WireProtocol {
    match app_type {
        AppType::Codex => codex_wire_protocol(provider),
        AppType::Claude | AppType::ClaudeDesktop => {
            if claude_api_format_needs_transform(get_claude_api_format(provider)) {
                claude_wire_protocol(provider)
            } else {
                WireProtocol::Native
            }
        }
        _ => WireProtocol::Native,
    }
}

/// Resolve the full capability snapshot for a provider on a given endpoint.
///
/// Declared capabilities (from `meta.capabilities`) always win. Otherwise we
/// derive from the resolved wire protocol using the same predicates that drive
/// routing; anything we cannot honestly infer stays `Unknown`.
pub fn resolve_capabilities(app_type: &AppType, provider: &Provider) -> ProviderCapabilitySnapshot {
    let declared = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.capabilities.as_ref());

    let protocol = resolve_wire_protocol(app_type, provider);

    let is_responses = matches!(protocol, WireProtocol::Responses);
    let is_chat = matches!(protocol, WireProtocol::ChatCompletions);

    // supportsResponses / supportsChatCompletions are inferable from the
    // resolved protocol; declaration still wins.
    let supports_responses = CapabilityState::resolve(
        declared.and_then(|c| c.supports_responses),
        Some(is_responses),
    );
    let supports_chat_completions = CapabilityState::resolve(
        declared.and_then(|c| c.supports_chat_completions),
        // A Chat upstream obviously supports Chat; a native-Responses upstream
        // may or may not — we do not guess, only a positive is inferable.
        if is_chat { Some(true) } else { None },
    );

    // These we cannot infer from format alone without guessing by name, so they
    // stay Unknown unless declared. A native Responses provider is the only case
    // where previous_response_id / response item ids / encrypted reasoning are
    // even semantically applicable, so a native chain gets a Heuristic positive
    // that a declaration can still override.
    let supports_previous_response_id = CapabilityState::resolve(
        declared.and_then(|c| c.supports_previous_response_id),
        if is_responses { Some(true) } else { None },
    );
    let supports_response_item_ids = CapabilityState::resolve(
        declared.and_then(|c| c.supports_response_item_ids),
        if is_responses { Some(true) } else { None },
    );
    let supports_encrypted_reasoning =
        CapabilityState::resolve(declared.and_then(|c| c.supports_encrypted_reasoning), None);
    let supports_usage_metadata =
        CapabilityState::resolve(declared.and_then(|c| c.supports_usage_metadata), None);
    let supports_streaming =
        CapabilityState::resolve(declared.and_then(|c| c.supports_streaming), None);
    let supports_tools = CapabilityState::resolve(declared.and_then(|c| c.supports_tools), None);
    let supports_reasoning =
        CapabilityState::resolve(declared.and_then(|c| c.supports_reasoning), None);

    // Continuation flavor: declared unsupported wins; otherwise native Responses
    // = server-side continuation eligible, Chat/Anthropic = degraded replay.
    let continuation = match declared.and_then(|c| c.supports_continuation) {
        Some(false) => ContinuationSupport::Unsupported,
        _ => {
            if is_responses {
                ContinuationSupport::Native
            } else if matches!(
                protocol,
                WireProtocol::ChatCompletions | WireProtocol::Anthropic
            ) {
                ContinuationSupport::Degraded
            } else {
                ContinuationSupport::Unknown
            }
        }
    };

    ProviderCapabilitySnapshot {
        protocol,
        supports_responses,
        supports_chat_completions,
        supports_streaming,
        supports_tools,
        supports_reasoning,
        supports_previous_response_id,
        continuation,
        supports_response_item_ids,
        supports_encrypted_reasoning,
        supports_usage_metadata,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn codex_provider(settings: serde_json::Value) -> Provider {
        Provider {
            id: "p1".to_string(),
            name: "P1".to_string(),
            settings_config: settings,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn native_responses_provider_reports_responses_and_native_continuation() {
        let provider = codex_provider(json!({ "base_url": "https://relay.example.com/v1" }));
        let snap = resolve_capabilities(&AppType::Codex, &provider);
        assert_eq!(snap.protocol, WireProtocol::Responses);
        assert!(snap.supports_responses.is_supported());
        assert_eq!(snap.continuation, ContinuationSupport::Native);
        // Chat is neither declared nor a positive inference here.
        assert_eq!(snap.supports_chat_completions, CapabilityState::Unknown);
    }

    #[test]
    fn chat_upstream_reports_chat_and_degraded_continuation() {
        let provider =
            codex_provider(json!({ "base_url": "https://relay.example.com/v1/chat/completions" }));
        let snap = resolve_capabilities(&AppType::Codex, &provider);
        assert_eq!(snap.protocol, WireProtocol::ChatCompletions);
        assert!(snap.supports_chat_completions.is_supported());
        assert_eq!(snap.continuation, ContinuationSupport::Degraded);
        // Native-only capabilities are not inferred for a Chat chain.
        assert_eq!(snap.supports_previous_response_id, CapabilityState::Unknown);
    }

    #[test]
    fn anthropic_wire_api_reports_anthropic_and_degraded() {
        let provider = codex_provider(json!({ "apiFormat": "anthropic" }));
        let snap = resolve_capabilities(&AppType::Codex, &provider);
        assert_eq!(snap.protocol, WireProtocol::Anthropic);
        assert_eq!(snap.continuation, ContinuationSupport::Degraded);
    }

    #[test]
    fn declared_capability_overrides_heuristic() {
        let mut provider = codex_provider(json!({ "base_url": "https://relay.example.com/v1" }));
        provider.meta = Some(crate::provider::ProviderMeta {
            capabilities: Some(crate::provider::ProviderCapabilities {
                supports_continuation: Some(false),
                supports_streaming: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        });

        let snap = resolve_capabilities(&AppType::Codex, &provider);
        // Native protocol would imply Native continuation, but the explicit
        // declaration of `false` must win.
        assert_eq!(snap.continuation, ContinuationSupport::Unsupported);
        assert!(matches!(
            snap.supports_streaming,
            CapabilityState::Supported {
                confidence: CapabilityConfidence::Declared
            }
        ));
    }

    #[test]
    fn undeclared_non_inferable_capabilities_stay_unknown() {
        let provider = codex_provider(json!({ "base_url": "https://relay.example.com/v1" }));
        let snap = resolve_capabilities(&AppType::Codex, &provider);
        // Streaming/tools/reasoning/usage cannot be inferred from format alone.
        assert_eq!(snap.supports_streaming, CapabilityState::Unknown);
        assert_eq!(snap.supports_tools, CapabilityState::Unknown);
        assert_eq!(snap.supports_reasoning, CapabilityState::Unknown);
        assert_eq!(snap.supports_usage_metadata, CapabilityState::Unknown);
        assert_eq!(snap.supports_encrypted_reasoning, CapabilityState::Unknown);
    }
}

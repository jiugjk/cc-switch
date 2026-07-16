//! Precise upstream quota / credit exhaustion classification.
//!
//! Coarse status-code buckets already send HTTP 429 to the next provider in the
//! failover chain (see `RequestForwarder::categorize_proxy_error`), so quota
//! exhaustion *already* fails over today. What was missing is a way to tell a
//! genuine "you are out of quota/credits" signal apart from an ordinary rate
//! limit or a transient upstream error, so that:
//!
//! - the event can be logged/surfaced (desensitized) as a distinct quota
//!   fallback rather than a generic retry, and
//! - an opt-in single-shot fallback can be gated to *exactly* explicit quota
//!   errors, never to the ambiguous cases the task enumerates as forbidden
//!   (timeouts, DNS/TLS, auth failures, generic 5xx, model-not-found, bad
//!   params, content-safety refusals, context-overflow, client cancels).
//!
//! The classifier is deliberately conservative: it only returns `true` on an
//! explicit quota/credit marker in the upstream error body (or an unambiguous
//! payment-required status). Anything it is not sure about is NOT a quota error.

use super::error::ProxyError;
use serde_json::Value;

/// Explicit quota/credit-exhaustion markers, matched case-insensitively against
/// the upstream error body's `code` / `type` / `message`-ish fields.
///
/// These are the canonical strings the task calls out plus close variants used
/// by common OpenAI-compatible relays. New markers should be added here rather
/// than widening the match to substrings that could catch unrelated errors.
const QUOTA_MARKERS: &[&str] = &[
    "quota_exceeded",
    "insufficient_quota",
    "usage_limit_reached",
    "usage_limit_exceeded",
    "credits_exhausted",
    "credit_exhausted",
    "insufficient_credits",
    "insufficient_credit",
    "billing_hard_limit_reached",
    "account_deactivated_insufficient_funds",
];

/// Markers that look quota-ish but must NOT be treated as quota exhaustion,
/// because they are ordinary transient rate limits (a retry may well succeed on
/// the same provider) rather than an exhausted balance. Kept separate so a bare
/// `rate_limit_exceeded` 429 stays a generic retry, not a quota fallback.
fn is_pure_rate_limit_marker(marker: &str) -> bool {
    matches!(
        marker,
        "rate_limit_exceeded"
            | "rate_limit"
            | "requests_rate_limit_exceeded"
            | "tokens_rate_limit_exceeded"
    )
}

/// Whether an upstream error unambiguously indicates quota / credit exhaustion.
///
/// Only `UpstreamError` can be a quota signal: network/timeout/TLS/DNS failures
/// surface as `Timeout` / `ForwardFailed` and are never classified as quota.
pub fn is_quota_exhaustion(error: &ProxyError) -> bool {
    let ProxyError::UpstreamError { status, body } = error else {
        return false;
    };

    // Body-driven detection is authoritative. A status code alone is never
    // enough (429 is usually a rate limit; 403 is usually auth), except 402
    // Payment Required, whose sole meaning is billing/quota.
    if let Some(body) = body {
        if let Some(hit) = body_has_quota_marker(body) {
            return hit;
        }
    }

    // 402 Payment Required with no contradicting body: treat as quota/billing.
    *status == 402
}

/// Scan an upstream error body for an explicit quota marker.
///
/// Returns:
/// - `Some(true)`  when a quota/credit-exhaustion marker is present,
/// - `Some(false)` when a pure rate-limit marker is present (explicitly NOT quota),
/// - `None`        when nothing conclusive is found (caller may fall back to status).
fn body_has_quota_marker(body: &str) -> Option<bool> {
    // Prefer structured fields to avoid matching quota words inside unrelated
    // free-text; fall back to a bounded scan of the raw body otherwise.
    if let Ok(json) = serde_json::from_str::<Value>(body) {
        let mut saw_rate_limit = false;
        for field in collect_error_signal_fields(&json) {
            let lowered = field.to_ascii_lowercase();
            if QUOTA_MARKERS.iter().any(|m| lowered.contains(m)) {
                return Some(true);
            }
            if is_pure_rate_limit_marker(lowered.trim()) {
                saw_rate_limit = true;
            }
        }
        if saw_rate_limit {
            return Some(false);
        }
        return None;
    }

    // Non-JSON body: check the canonical markers as substrings, but only the
    // exhaustion ones (never widen to rate-limit here).
    let lowered = body.to_ascii_lowercase();
    if QUOTA_MARKERS.iter().any(|m| lowered.contains(m)) {
        return Some(true);
    }
    None
}

/// Pull the fields that carry an upstream error's machine code / type / message
/// from the common OpenAI / Anthropic / relay error envelope shapes.
fn collect_error_signal_fields(json: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |v: Option<&Value>| {
        if let Some(s) = v.and_then(Value::as_str) {
            out.push(s.to_string());
        }
    };

    // { "error": { "code": .., "type": .., "message": .. } }
    if let Some(err) = json.get("error") {
        push(err.get("code"));
        push(err.get("type"));
        push(err.get("message"));
        push(err.get("status"));
    }
    // Top-level variants used by some relays.
    push(json.get("code"));
    push(json.get("type"));
    push(json.get("message"));
    push(json.get("detail"));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream(status: u16, body: &str) -> ProxyError {
        ProxyError::UpstreamError {
            status,
            body: Some(body.to_string()),
        }
    }

    #[test]
    fn detects_canonical_quota_codes() {
        for code in [
            "quota_exceeded",
            "insufficient_quota",
            "usage_limit_reached",
            "credits_exhausted",
        ] {
            let body = format!(r#"{{"error":{{"code":"{code}","message":"nope"}}}}"#);
            assert!(
                is_quota_exhaustion(&upstream(429, &body)),
                "{code} must classify as quota"
            );
        }
    }

    #[test]
    fn detects_quota_in_type_and_message_fields() {
        assert!(is_quota_exhaustion(&upstream(
            403,
            r#"{"error":{"type":"insufficient_quota","message":"x"}}"#
        )));
        assert!(is_quota_exhaustion(&upstream(
            200, // body-driven; status irrelevant when marker present
            r#"{"error":{"message":"You have hit your usage_limit_reached for this month"}}"#
        )));
    }

    #[test]
    fn payment_required_without_body_is_quota() {
        assert!(is_quota_exhaustion(&ProxyError::UpstreamError {
            status: 402,
            body: None,
        }));
    }

    #[test]
    fn pure_rate_limit_is_not_quota() {
        // A plain 429 rate-limit must remain a generic retry, not a quota fallback.
        assert!(!is_quota_exhaustion(&upstream(
            429,
            r#"{"error":{"code":"rate_limit_exceeded","message":"slow down"}}"#
        )));
        // Bare 429 with an opaque body is not conclusively quota.
        assert!(!is_quota_exhaustion(&upstream(429, "Too Many Requests")));
    }

    #[test]
    fn forbidden_false_positives_never_classify_as_quota() {
        // Timeouts / network / TLS surface as non-UpstreamError variants.
        assert!(!is_quota_exhaustion(&ProxyError::Timeout("60s".into())));
        assert!(!is_quota_exhaustion(&ProxyError::ForwardFailed(
            "dns error".into()
        )));
        assert!(!is_quota_exhaustion(&ProxyError::AuthError(
            "bad key".into()
        )));
        // Auth / model / param / content errors as upstream bodies must not match.
        assert!(!is_quota_exhaustion(&upstream(
            401,
            r#"{"error":{"code":"invalid_api_key"}}"#
        )));
        assert!(!is_quota_exhaustion(&upstream(
            404,
            r#"{"error":{"code":"model_not_found"}}"#
        )));
        assert!(!is_quota_exhaustion(&upstream(
            400,
            r#"{"error":{"code":"invalid_request_error"}}"#
        )));
        assert!(!is_quota_exhaustion(&upstream(
            500,
            r#"{"error":{"message":"internal server error"}}"#
        )));
    }

    #[test]
    fn non_json_body_matches_only_exhaustion_markers() {
        assert!(is_quota_exhaustion(&upstream(
            429,
            "error: insufficient_quota for this account"
        )));
        assert!(!is_quota_exhaustion(&upstream(
            429,
            "error: rate_limit_exceeded, retry later"
        )));
    }
}

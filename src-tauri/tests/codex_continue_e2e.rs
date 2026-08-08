//! End-to-end integration test for the CodexCont reasoning auto-continuation
//! feature, driven through the real local proxy.
//!
//! TEST 1 (`codex_continue_folds_truncated_responses_into_single_stream`):
//!   A truncated streaming `/v1/responses` request is automatically continued
//!   and folded into ONE SSE stream. Proves the continuation (2nd) upstream
//!   request replays the reasoning item (incl. `encrypted_content`), forces
//!   `include: ["reasoning.encrypted_content"]`, injects the commentary marker,
//!   and drops `previous_response_id`.
//!
//! TEST 2 (`codex_continue_disabled_when_chain_contains_converting_provider`):
//!   When the failover candidate chain contains a provider that converts
//!   Responses -> Chat, the whole-chain gate disables continuation entirely:
//!   exactly one upstream request happens and the initial body is NOT augmented
//!   with the `include` injection.
//!
//! The proxy internals (`ProxyServer`, `ProxyConfig`, `CodexContinueConfig`) are
//! private to the crate, so the proxy is driven through the public
//! `AppState`/`ProxyService` surface, and `max_continuations` is bounded through
//! the documented `CCSWITCH_CODEX_CONTINUE_MAX` env override rather than the DAO
//! setter (whose argument type cannot be named from an integration test).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{extract::State, response::Response, Router};
use cc_switch_lib::{AppState, Database, Provider};
use serde_json::{json, Value};

#[path = "support.rs"]
mod support;

/// Round 1 fixture: a `response.completed` whose
/// `usage.output_tokens_details.reasoning_tokens == 516` (== 518*1 - 2) trips the
/// truncation fingerprint, so the proxy continues.
const R1: &[u8] = include_bytes!("fixtures/codex_poc_r1.sse.txt");
/// Round 2 fixture. NOTE: its `reasoning_tokens == 2588` (== 518*5 - 2) ALSO
/// matches the truncation fingerprint, so continuation is bounded to a single
/// round via `CCSWITCH_CODEX_CONTINUE_MAX=1` to keep TEST 1 at exactly 2 upstream
/// requests.
const R2: &[u8] = include_bytes!("fixtures/codex_poc_r2.sse.txt");
/// Tool-call round fixture: reasoning (`encrypted_content` present) followed by a
/// completed `function_call`, with `reasoning_tokens == 516` — the truncation
/// fingerprint matches a legitimately COMPLETE tool-call response. TEST 3 proves
/// such a round is never continued (the call must reach the client, not be
/// swallowed by a continuation).
const R_TOOL: &[u8] = include_bytes!("fixtures/codex_tool_round.sse.txt");
/// A normally completed reasoning round whose token count does not match the
/// truncation fingerprint. TEST 4 proves the fold still emits the complete
/// reasoning and message output without issuing a continuation request.
const R_COMPLETE: &[u8] = include_bytes!("fixtures/codex_complete_round.sse.txt");

/// The default continuation marker (mirrors `codex_continue::DEFAULT_MARKER`,
/// which is private).
const DEFAULT_MARKER: &str =
    "We need continue thinking. Do not summarize; continue from the previous reasoning state.";

// ============================================================================
// Mock upstream (native Codex Responses provider)
// ============================================================================

struct MockUpstream {
    /// Raw request bodies received, in arrival order.
    bodies: Mutex<Vec<Value>>,
    /// Number of requests served so far (selects which fixture to return).
    served: AtomicUsize,
    /// SSE fixtures served per request index; the last one repeats for any
    /// additional requests.
    fixtures: Vec<&'static [u8]>,
}

async fn mock_responses_handler(
    State(state): State<Arc<MockUpstream>>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.bodies.lock().expect("mock bodies lock").push(parsed);

    let index = state.served.fetch_add(1, Ordering::SeqCst);
    let fixture: &[u8] = state.fixtures[index.min(state.fixtures.len() - 1)];

    Response::builder()
        .status(200)
        .header(
            axum::http::header::CONTENT_TYPE,
            "text/event-stream; charset=utf-8",
        )
        .body(axum::body::Body::from(fixture.to_vec()))
        .expect("build mock SSE response")
}

/// Bind an ephemeral in-test upstream and return `(port, shared_state)`.
async fn start_mock_upstream(fixtures: Vec<&'static [u8]>) -> (u16, Arc<MockUpstream>) {
    let state = Arc::new(MockUpstream {
        bodies: Mutex::new(Vec::new()),
        served: AtomicUsize::new(0),
        fixtures,
    });
    // A fallback route accepts any method/path, so it does not matter how the
    // forwarder assembles the upstream URL (`/v1/responses`).
    let app = Router::new()
        .fallback(mock_responses_handler)
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let port = listener.local_addr().expect("mock local addr").port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (port, state)
}

// ============================================================================
// Helpers
// ============================================================================

/// Pin the CodexCont config to a deterministic shape regardless of ambient env:
/// enabled, `max_continuations = max`, default step (518) and default marker.
fn set_codex_continue_env(max: &str) {
    std::env::set_var("CCSWITCH_CODEX_CONTINUE", "1");
    std::env::set_var("CCSWITCH_CODEX_CONTINUE_MAX", max);
    std::env::remove_var("CCSWITCH_CODEX_CONTINUE_STEP");
    std::env::remove_var("CCSWITCH_CODEX_CONTINUE_MARKER");
}

/// A native Codex Responses provider pointing at the mock. `base_url` ends in
/// `/v1`, so `CodexAdapter::build_url` resolves `/responses` to
/// `http://127.0.0.1:{port}/v1/responses`.
fn native_codex_provider(id: &str, mock_port: u16, sort_index: usize) -> Provider {
    let mut provider = Provider::with_id(
        id.to_string(),
        "Native Codex".to_string(),
        json!({
            "base_url": format!("http://127.0.0.1:{mock_port}/v1"),
            "auth": { "OPENAI_API_KEY": "sk-native-test" }
        }),
        None,
    );
    provider.category = Some("codex".to_string());
    provider.sort_index = Some(sort_index);
    provider
}

/// A Codex provider that converts Responses -> Chat Completions
/// (`apiFormat = "openai_chat"` makes `should_convert_codex_responses_to_chat`
/// return true). Its upstream is never contacted in TEST 2.
fn converting_codex_provider(id: &str, sort_index: usize) -> Provider {
    let mut provider = Provider::with_id(
        id.to_string(),
        "Converting Codex".to_string(),
        json!({
            "apiFormat": "openai_chat",
            "base_url": "http://127.0.0.1:1/v1",
            "auth": { "OPENAI_API_KEY": "sk-converting-test" }
        }),
        None,
    );
    provider.category = Some("codex".to_string());
    provider.sort_index = Some(sort_index);
    provider
}

/// Configure the DB proxy port to an ephemeral one, start the proxy, and return
/// the bound port.
async fn start_proxy(state: &AppState) -> u16 {
    let mut proxy_config = state
        .db
        .get_proxy_config()
        .await
        .expect("read proxy config");
    proxy_config.listen_port = 0;
    state
        .db
        .update_proxy_config(proxy_config)
        .await
        .expect("use ephemeral proxy port");

    state
        .proxy_service
        .start()
        .await
        .expect("start local proxy")
        .port
}

/// A Codex-CLI-style streaming `/v1/responses` body. `include` is intentionally
/// absent so the initial-payload injection is observable; `previous_response_id`
/// is present so the continuation payload can be shown to drop it.
fn codex_request_body() -> Value {
    json!({
        "model": "gpt-5.5",
        "stream": true,
        "instructions": "You are a helpful coding agent.",
        "previous_response_id": "resp_prev_test",
        "reasoning": { "effort": "high" },
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "candy counting puzzle" }]
            }
        ]
    })
}

/// POST to the proxy `/v1/responses` endpoint and read the full response with a
/// bounded timeout. Returns `(status, content_type, body_bytes)`.
async fn post_responses(
    proxy_port: u16,
    body: &Value,
) -> (reqwest::StatusCode, String, bytes::Bytes) {
    // `.no_proxy()` keeps the client->proxy hop direct regardless of ambient
    // proxy env vars.
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("build test client");
    let url = format!("http://127.0.0.1:{proxy_port}/v1/responses");

    tokio::time::timeout(Duration::from_secs(20), async {
        let resp = client
            .post(&url)
            .header("accept", "text/event-stream")
            .json(body)
            .send()
            .await
            .expect("send request to proxy");
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body_bytes = resp.bytes().await.expect("read proxy response body");
        (status, content_type, body_bytes)
    })
    .await
    .expect("proxy responded within 20s")
}

/// Parse an SSE stream into its JSON event payloads (skips `[DONE]` and
/// comment/keepalive-only blocks).
fn parse_sse_events(body: &str) -> Vec<Value> {
    let mut events = Vec::new();
    for block in body.split("\n\n") {
        let mut data_lines: Vec<&str> = Vec::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
            }
        }
        if data_lines.is_empty() {
            continue;
        }
        let payload = data_lines.join("\n");
        if payload.trim() == "[DONE]" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&payload) {
            events.push(value);
        }
    }
    events
}

fn contains_str(array: &Value, needle: &str) -> bool {
    array
        .as_array()
        .map(|items| items.iter().any(|v| v.as_str() == Some(needle)))
        .unwrap_or(false)
}

fn non_empty_encrypted(item: &Value) -> bool {
    item.get("encrypted_content")
        .and_then(Value::as_str)
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

// ============================================================================
// TEST 1 — happy path fold
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(
    clippy::await_holding_lock,
    reason = "serialize process-global test HOME, settings cache, and CodexCont env overrides across async proxy calls"
)]
async fn codex_continue_folds_truncated_responses_into_single_stream() {
    let _guard = support::test_mutex().lock().expect("acquire test mutex");
    support::reset_test_fs();
    let _home = support::ensure_test_home();
    set_codex_continue_env("1");

    let (mock_port, mock) = start_mock_upstream(vec![R1, R2]).await;

    let db = Arc::new(Database::memory().expect("in-memory database"));
    let native = native_codex_provider("native-codex", mock_port, 0);
    db.save_provider("codex", &native)
        .expect("save native provider");
    db.set_current_provider("codex", "native-codex")
        .expect("set current provider");
    // Failover stays disabled (default), so the candidate chain is just the
    // single native provider and the whole-chain gate allows continuation.

    let state = AppState::new(db);
    let proxy_port = start_proxy(&state).await;

    let (status, content_type, body_bytes) =
        post_responses(proxy_port, &codex_request_body()).await;

    // --- folded client stream: transport-level guarantees ---
    assert_eq!(status, reqwest::StatusCode::OK, "folded response is 200");
    assert!(
        content_type.contains("text/event-stream"),
        "folded response is SSE, got content-type: {content_type}"
    );

    let body = String::from_utf8_lossy(&body_bytes);
    let events = parse_sse_events(&body);

    let created = events
        .iter()
        .filter(|e| e["type"] == "response.created")
        .count();
    assert_eq!(created, 1, "exactly one response.created after folding");

    let completed: Vec<&Value> = events
        .iter()
        .filter(|e| e["type"] == "response.completed")
        .collect();
    assert_eq!(
        completed.len(),
        1,
        "exactly one terminal response.completed after folding"
    );

    // sequence_number must be present on every event and strictly increasing
    // from 0 across the whole folded stream.
    let seqs: Vec<u64> = events
        .iter()
        .map(|e| {
            e["sequence_number"]
                .as_u64()
                .expect("every folded event carries a sequence_number")
        })
        .collect();
    assert_eq!(seqs.first().copied(), Some(0), "sequence starts at 0");
    for window in seqs.windows(2) {
        assert!(
            window[1] > window[0],
            "sequence_number strictly increasing, saw {} then {}",
            window[0],
            window[1]
        );
    }

    // --- folded terminal: reconstructed response ---
    let response = &completed[0]["response"];
    assert_eq!(response["status"], "completed");

    // Public usage folds both rounds:
    //   input_tokens  = first round input (4582)
    //   reasoning     = 516 (r1) + 2588 (r2) = 3104
    //   output_tokens = total reasoning (3104) + final visible output
    //                   (2947 - 2588 = 359) = 3463
    //   total_tokens  = 4582 + 3463 = 8045
    let usage = &response["usage"];
    assert_eq!(usage["input_tokens"], json!(4582), "public input tokens");
    assert_eq!(usage["output_tokens"], json!(3463), "folded public output");
    assert_eq!(usage["total_tokens"], json!(8045), "folded total");
    assert_eq!(
        usage["output_tokens_details"]["reasoning_tokens"],
        json!(3104),
        "reasoning tokens summed across both rounds"
    );
    assert_eq!(
        usage["input_tokens_details"]["cached_tokens"],
        json!(3840),
        "cached tokens taken from the first round"
    );

    // Folded output = [round-1 reasoning, round-2 reasoning, round-2 message].
    // (Round 1's message item is intentionally dropped by the fold when the
    // round continues; only reasoning items are replayed forward.)
    let output = response["output"].as_array().expect("output array");
    assert_eq!(
        output.len(),
        3,
        "reasoning from both rounds plus the final round's message"
    );
    assert_eq!(output[0]["type"], "reasoning");
    assert!(
        non_empty_encrypted(&output[0]),
        "round-1 reasoning keeps encrypted_content"
    );
    assert!(
        serde_json::to_string(&output[0]["summary"])
            .unwrap()
            .contains("Evaluating candy selection"),
        "round-1 reasoning summary folded into output"
    );
    assert_eq!(output[1]["type"], "reasoning");
    assert!(
        non_empty_encrypted(&output[1]),
        "round-2 reasoning keeps encrypted_content"
    );
    assert!(
        serde_json::to_string(&output[1]["summary"])
            .unwrap()
            .contains("Ensuring conditions for success"),
        "round-2 reasoning summary folded into output"
    );
    assert_eq!(output[2]["type"], "message");
    assert!(
        output[2]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("21"),
        "final visible answer comes from round 2"
    );

    // --- upstream requests recorded by the mock ---
    let bodies = mock.bodies.lock().expect("mock bodies lock");
    assert_eq!(
        bodies.len(),
        2,
        "exactly two upstream requests (1 continuation)"
    );

    let initial = &bodies[0];
    // The initial payload keeps previous_response_id but has the encrypted
    // include injected (prepare_initial_payload).
    assert_eq!(
        initial["previous_response_id"], "resp_prev_test",
        "round 1 keeps the client's previous_response_id"
    );
    assert!(
        contains_str(&initial["include"], "reasoning.encrypted_content"),
        "round 1 forces include reasoning.encrypted_content"
    );

    let continuation = &bodies[1];
    assert!(
        continuation.get("previous_response_id").is_none(),
        "continuation request drops previous_response_id"
    );
    assert!(
        contains_str(&continuation["include"], "reasoning.encrypted_content"),
        "continuation forces include reasoning.encrypted_content"
    );
    assert_eq!(
        continuation["stream"],
        json!(true),
        "continuation is streamed"
    );

    let continuation_input = continuation["input"]
        .as_array()
        .expect("continuation input array");
    assert!(
        continuation_input
            .iter()
            .any(|item| item["type"] == "reasoning" && non_empty_encrypted(item)),
        "continuation replays a reasoning item with encrypted_content"
    );
    assert!(
        continuation_input.iter().any(|item| {
            item["type"] == "message"
                && item["role"] == "assistant"
                && item["phase"] == "commentary"
                && item["content"][0]["text"].as_str() == Some(DEFAULT_MARKER)
        }),
        "continuation injects the commentary marker with the default text"
    );

    let _ = state.proxy_service.stop().await;
}

// ============================================================================
// TEST 2 — converting provider in the chain disables continuation
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(
    clippy::await_holding_lock,
    reason = "serialize process-global test HOME, settings cache, and CodexCont env overrides across async proxy calls"
)]
async fn codex_continue_disabled_when_chain_contains_converting_provider() {
    let _guard = support::test_mutex().lock().expect("acquire test mutex");
    support::reset_test_fs();
    let _home = support::ensure_test_home();
    // CodexCont is enabled at the config level; the ONLY thing that must disable
    // continuation here is the whole-chain gate.
    set_codex_continue_env("1");

    let (mock_port, mock) = start_mock_upstream(vec![R1, R2]).await;

    let db = Arc::new(Database::memory().expect("in-memory database"));
    let native = native_codex_provider("native-codex", mock_port, 0);
    let converting = converting_codex_provider("converting-codex", 1);
    db.save_provider("codex", &native)
        .expect("save native provider");
    db.save_provider("codex", &converting)
        .expect("save converting provider");
    db.set_current_provider("codex", "native-codex")
        .expect("set current provider");

    // Enable failover and build the candidate chain [native, converting]. The
    // request is served by the native provider (sort_index 0, tried first), but
    // the converting provider's presence in the chain must disable continuation.
    let mut app_config = db
        .get_proxy_config_for_app("codex")
        .await
        .expect("read codex app proxy config");
    app_config.auto_failover_enabled = true;
    db.update_proxy_config_for_app(app_config)
        .await
        .expect("enable codex auto failover");
    db.add_to_failover_queue("codex", "native-codex")
        .expect("queue native provider");
    db.add_to_failover_queue("codex", "converting-codex")
        .expect("queue converting provider");

    let state = AppState::new(db);
    let proxy_port = start_proxy(&state).await;

    let (status, _content_type, body_bytes) =
        post_responses(proxy_port, &codex_request_body()).await;

    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "native provider passthrough is 200"
    );
    // Drain (round 1 is passed through unfolded); body content is not asserted.
    let _ = String::from_utf8_lossy(&body_bytes);

    let bodies = mock.bodies.lock().expect("mock bodies lock");
    assert_eq!(
        bodies.len(),
        1,
        "a converting provider in the chain disables continuation (no folding, no continuation request) despite the truncation fingerprint"
    );

    let initial = &bodies[0];
    assert!(
        initial.get("include").is_none(),
        "continuation gated off -> the initial upstream body is NOT augmented with an include injection, got: {}",
        initial
    );

    let _ = state.proxy_service.stop().await;
}

// ============================================================================
// TEST 3 — a round carrying a completed tool call is never continued
// ============================================================================

/// Regression test for the "model says it has no exec/shell tools this turn"
/// class: a legitimately COMPLETE response ending in a `function_call` whose
/// `reasoning_tokens` coincidentally matches the 518n-2 truncation fingerprint
/// (~1/step of tool-call rounds) used to trigger a continuation that silently
/// swallowed the tool call (never streamed downstream, absent from the folded
/// terminal output, not replayed upstream). The fix gates continuation on the
/// round having no buffered non-message output items.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(
    clippy::await_holding_lock,
    reason = "serialize process-global test HOME, settings cache, and CodexCont env overrides across async proxy calls"
)]
async fn codex_continue_never_swallows_a_tool_call_round() {
    let _guard = support::test_mutex().lock().expect("acquire test mutex");
    support::reset_test_fs();
    let _home = support::ensure_test_home();
    // Continuation budget is available; only the pending-tool-output gate may
    // stop the fold from issuing a second upstream request.
    set_codex_continue_env("8");

    let (mock_port, mock) = start_mock_upstream(vec![R_TOOL]).await;

    let db = Arc::new(Database::memory().expect("in-memory database"));
    let native = native_codex_provider("native-codex", mock_port, 0);
    db.save_provider("codex", &native)
        .expect("save native provider");
    db.set_current_provider("codex", "native-codex")
        .expect("set current provider");

    let state = AppState::new(db);
    let proxy_port = start_proxy(&state).await;

    let (status, content_type, body_bytes) =
        post_responses(proxy_port, &codex_request_body()).await;

    assert_eq!(status, reqwest::StatusCode::OK, "folded response is 200");
    assert!(
        content_type.contains("text/event-stream"),
        "folded response is SSE, got content-type: {content_type}"
    );

    let body = String::from_utf8_lossy(&body_bytes);
    let events = parse_sse_events(&body);

    // The client stream must carry the full function_call event chain.
    let call_added = events
        .iter()
        .any(|e| e["type"] == "response.output_item.added" && e["item"]["type"] == "function_call");
    assert!(
        call_added,
        "function_call output_item.added reaches the client"
    );
    let args_deltas: Vec<&Value> = events
        .iter()
        .filter(|e| e["type"] == "response.function_call_arguments.delta")
        .collect();
    assert_eq!(
        args_deltas.len(),
        2,
        "both buffered argument deltas are flushed in order"
    );
    assert_eq!(args_deltas[0]["delta"], json!("{\"command\":[\"ls\""));
    assert_eq!(args_deltas[1]["delta"], json!(",\"-la\"]}"));
    let call_done = events
        .iter()
        .find(|e| e["type"] == "response.output_item.done" && e["item"]["type"] == "function_call")
        .expect("function_call output_item.done reaches the client");
    assert_eq!(call_done["item"]["call_id"], "call_shell_1");
    assert_eq!(call_done["item"]["name"], "shell");
    assert_eq!(
        call_done["item"]["arguments"],
        json!("{\"command\":[\"ls\",\"-la\"]}"),
        "tool call arguments survive the fold verbatim"
    );

    // Terminal output preserves [reasoning, function_call] with identity intact.
    let completed: Vec<&Value> = events
        .iter()
        .filter(|e| e["type"] == "response.completed")
        .collect();
    assert_eq!(completed.len(), 1, "exactly one terminal event");
    let response = &completed[0]["response"];
    let output = response["output"].as_array().expect("output array");
    assert_eq!(output.len(), 2, "reasoning + function_call in final output");
    assert_eq!(output[0]["type"], "reasoning");
    assert!(
        non_empty_encrypted(&output[0]),
        "reasoning keeps encrypted_content"
    );
    assert_eq!(output[1]["type"], "function_call");
    assert_eq!(output[1]["call_id"], "call_shell_1");
    assert_eq!(output[1]["name"], "shell");

    // Diagnosability: the fold metadata records WHY it refused to continue.
    let continue_md = &response["metadata"]["ccswitch_codex_continue"];
    assert_eq!(
        continue_md["stopped_reason"],
        json!("pending_tool_output"),
        "stopped_reason explains the pending tool call"
    );
    let rounds = response["metadata"]["proxy_rounds"]
        .as_array()
        .expect("proxy_rounds array");
    assert_eq!(rounds.len(), 1, "single round recorded");
    assert_eq!(rounds[0]["truncated"], json!(true));
    assert_eq!(rounds[0]["pending_tool_output"], json!(true));
    assert_eq!(rounds[0]["continued"], json!(false));

    // Exactly ONE upstream request: the fingerprint match must NOT spawn a
    // continuation when the round carries a tool call.
    let bodies = mock.bodies.lock().expect("mock bodies lock");
    assert_eq!(
        bodies.len(),
        1,
        "tool-call round is never continued despite matching the truncation fingerprint"
    );

    let _ = state.proxy_service.stop().await;
}

// ============================================================================
// TEST 4 — a normally completed reasoning round is passed through once
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(
    clippy::await_holding_lock,
    reason = "serialize process-global test HOME, settings cache, and CodexCont env overrides across async proxy calls"
)]
async fn codex_continue_preserves_non_truncated_reasoning_round() {
    let _guard = support::test_mutex().lock().expect("acquire test mutex");
    support::reset_test_fs();
    let _home = support::ensure_test_home();
    set_codex_continue_env("8");

    let (mock_port, mock) = start_mock_upstream(vec![R_COMPLETE]).await;

    let db = Arc::new(Database::memory().expect("in-memory database"));
    let native = native_codex_provider("native-codex", mock_port, 0);
    db.save_provider("codex", &native)
        .expect("save native provider");
    db.set_current_provider("codex", "native-codex")
        .expect("set current provider");

    let state = AppState::new(db);
    let proxy_port = start_proxy(&state).await;

    let (status, content_type, body_bytes) =
        post_responses(proxy_port, &codex_request_body()).await;

    assert_eq!(status, reqwest::StatusCode::OK, "complete response is 200");
    assert!(
        content_type.contains("text/event-stream"),
        "complete response is SSE, got content-type: {content_type}"
    );

    let body = String::from_utf8_lossy(&body_bytes);
    let events = parse_sse_events(&body);
    let completed: Vec<&Value> = events
        .iter()
        .filter(|event| event["type"] == "response.completed")
        .collect();
    assert_eq!(
        completed.len(),
        1,
        "exactly one terminal response.completed"
    );

    let response = &completed[0]["response"];
    assert_eq!(response["status"], json!("completed"));
    let output = response["output"].as_array().expect("output array");
    assert_eq!(output.len(), 2, "reasoning and final message are preserved");
    assert_eq!(output[0]["type"], json!("reasoning"));
    assert!(non_empty_encrypted(&output[0]));
    assert_eq!(output[1]["type"], json!("message"));
    assert_eq!(
        output[1]["content"][0]["text"],
        json!("The answer is ready.")
    );

    let continue_md = &response["metadata"]["ccswitch_codex_continue"];
    assert_eq!(continue_md["stopped_reason"], Value::Null);
    let rounds = response["metadata"]["proxy_rounds"]
        .as_array()
        .expect("proxy_rounds array");
    assert_eq!(rounds.len(), 1);
    assert_eq!(rounds[0]["reasoning_tokens"], json!(20));
    assert_eq!(rounds[0]["truncated"], json!(false));
    assert_eq!(rounds[0]["continued"], json!(false));

    let bodies = mock.bodies.lock().expect("mock bodies lock");
    assert_eq!(bodies.len(), 1, "non-truncated reasoning is not continued");
    let input = bodies[0]["input"].as_array().expect("input array");
    assert!(
        input
            .iter()
            .all(|item| item["phase"] != json!("commentary")),
        "a non-truncated round does not inject a continuation marker"
    );

    let _ = state.proxy_service.stop().await;
}

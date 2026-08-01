//! Codex Responses reasoning continuation/folding for native `/v1/responses` SSE.
//!
//! This module is deliberately self-contained: provider selection, routing and
//! failover still happen through `RequestForwarder::forward_with_retry`.

use super::{
    forwarder::{ActiveConnectionGuard, RequestForwarder},
    hyper_client::ProxyResponse,
    providers::{provider_needs_responses_namespace_flatten, transform_codex_responses_namespace},
    sse::{append_utf8_safe, strip_sse_field, take_sse_block},
};
use crate::{app_config::AppType, provider::Provider};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use http::{
    header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING},
    Extensions, HeaderMap, HeaderValue, Method,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const DEFAULT_STEP: u64 = 518;
const DEFAULT_MAX_CONTINUATIONS: usize = 8;
pub(crate) const MAX_CONTINUATIONS: usize = 32;
const DEFAULT_MARKER: &str =
    "We need continue thinking. Do not summarize; continue from the previous reasoning state.";
const ENCRYPTED_INCLUDE: &str = "reasoning.encrypted_content";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexContinueConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_continuations")]
    pub max_continuations: usize,
    #[serde(default = "default_step")]
    pub step: u64,
    #[serde(default = "default_marker")]
    pub marker: String,
}

impl CodexContinueConfig {
    pub(crate) fn from_settings_with_env(settings: Self) -> Self {
        Self {
            enabled: env_bool_override("CCSWITCH_CODEX_CONTINUE", settings.enabled),
            max_continuations: env_usize_override(
                "CCSWITCH_CODEX_CONTINUE_MAX",
                settings.max_continuations,
            ),
            step: env_u64_override("CCSWITCH_CODEX_CONTINUE_STEP", settings.step).max(3),
            marker: std::env::var("CCSWITCH_CODEX_CONTINUE_MARKER")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(settings.marker),
        }
        .normalized()
    }

    pub(crate) fn from_env() -> Self {
        Self::from_settings_with_env(Self::default())
    }

    pub(crate) fn normalized(mut self) -> Self {
        self.max_continuations = self.max_continuations.min(MAX_CONTINUATIONS);
        self.step = self.step.max(3);
        if self.marker.trim().is_empty() {
            self.marker = default_marker();
        }
        self
    }
}

impl Default for CodexContinueConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_continuations: default_max_continuations(),
            step: default_step(),
            marker: default_marker(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_max_continuations() -> usize {
    DEFAULT_MAX_CONTINUATIONS
}

fn default_step() -> u64 {
    DEFAULT_STEP
}

fn default_marker() -> String {
    DEFAULT_MARKER.to_string()
}

fn env_bool_override(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "0" | "false" | "off" | "no" => false,
            "1" | "true" | "on" | "yes" => true,
            _ => default,
        },
        Err(_) => default,
    }
}

fn env_usize_override(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_u64_override(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

/// Request gate: only native streaming Responses requests with reasoning enabled.
///
/// Provider/endpoint gates are handled by `handlers.rs`: this must run only in
/// the native Responses branch, never in Chat conversion or compact.
pub(crate) fn should_enable_for_request(body: &Value, config: &CodexContinueConfig) -> bool {
    config.enabled
        && body.get("stream").and_then(Value::as_bool).unwrap_or(false)
        && !matches!(body.get("reasoning"), Some(Value::Bool(false)))
}

pub(crate) fn is_truncation_pattern(reasoning_tokens: Option<u64>, step: u64) -> bool {
    let Some(tokens) = reasoning_tokens else {
        return false;
    };
    let step = step.max(3);
    tokens >= step - 2 && (tokens + 2) % step == 0
}

fn reasoning_tokens(usage: Option<&Value>) -> Option<u64> {
    usage?
        .get("output_tokens_details")?
        .get("reasoning_tokens")?
        .as_u64()
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SseFrame {
    Event(Value),
    Done,
}

#[derive(Default, Debug)]
pub(crate) struct IncrementalSseParser {
    buffer: String,
    utf8_remainder: Vec<u8>,
}

impl IncrementalSseParser {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<SseFrame> {
        if bytes.is_empty() {
            return Vec::new();
        }

        append_utf8_safe(&mut self.buffer, &mut self.utf8_remainder, bytes);
        let mut out = Vec::new();
        while let Some(block) = take_sse_block(&mut self.buffer) {
            if let Some(frame) = parse_sse_block(&block) {
                out.push(frame);
            }
        }
        out
    }

    pub(crate) fn finish(&mut self) -> Vec<SseFrame> {
        if !self.utf8_remainder.is_empty() {
            self.buffer
                .push_str(&String::from_utf8_lossy(&self.utf8_remainder));
            self.utf8_remainder.clear();
        }
        let trailing = std::mem::take(&mut self.buffer);
        parse_sse_block(&trailing).into_iter().collect()
    }
}

fn parse_sse_block(block: &str) -> Option<SseFrame> {
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if line.starts_with(':') {
            continue;
        }
        if let Some(data) = strip_sse_field(line.trim_end_matches('\r'), "data") {
            data_lines.push(data.to_string());
        }
    }

    if data_lines.is_empty() {
        return None;
    }

    let payload = data_lines.join("\n");
    if payload.trim() == "[DONE]" {
        return Some(SseFrame::Done);
    }

    serde_json::from_str::<Value>(&payload)
        .ok()
        .map(SseFrame::Event)
}

fn sse_event(event: &Value) -> Bytes {
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    let data = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    Bytes::from(format!("event: {event_type}\ndata: {data}\n\n"))
}

fn sse_done() -> Bytes {
    Bytes::from_static(b"data: [DONE]\n\n")
}

fn set_sequence(event: &mut Value, seq: &mut u64) {
    if let Some(obj) = event.as_object_mut() {
        obj.insert("sequence_number".to_string(), json!(*seq));
        *seq += 1;
    }
}

fn next_sequence(seq: &mut u64) -> u64 {
    let current = *seq;
    *seq += 1;
    current
}

fn set_output_index(event: &mut Value, output_index: usize) {
    if let Some(obj) = event.as_object_mut() {
        if obj.contains_key("output_index") {
            obj.insert("output_index".to_string(), json!(output_index));
        }
    }
}

fn event_type(event: &Value) -> &str {
    event.get("type").and_then(Value::as_str).unwrap_or("")
}

fn terminal_event(event: &Value) -> bool {
    matches!(
        event_type(event),
        "response.completed" | "response.incomplete" | "response.failed"
    )
}

fn terminal_allows_continuation(event: &Value) -> bool {
    matches!(
        event_type(event),
        "response.completed" | "response.incomplete"
    ) && !matches!(
        event
            .get("response")
            .and_then(|response| response.get("status"))
            .and_then(Value::as_str),
        Some("failed" | "cancelled")
    )
}

fn round_terminal_allows_continuation(terminal: Option<&Value>, saw_error_event: bool) -> bool {
    !saw_error_event && terminal.is_some_and(terminal_allows_continuation)
}

fn output_index(event: &Value) -> Option<Value> {
    event.get("output_index").cloned()
}

fn output_item_type(event: &Value) -> Option<&str> {
    event
        .get("item")
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
}

fn has_encrypted_content(item: &Value) -> bool {
    item.get("encrypted_content")
        .and_then(Value::as_str)
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

fn usage_from_terminal(event: &Value) -> Option<&Value> {
    event.get("response").and_then(|r| r.get("usage"))
}

/// 截断续写只对「纯推理中断」的轮次安全。若本轮已缓冲了 message 之外的
/// 输出项（function_call / custom_tool_call / local_shell_call 等工具调用，
/// 或未知类型），说明模型已产出等待客户端执行的动作：此时继续续写会把
/// 这些缓冲项整个吞掉——既不下发、不进 final_output、也无法安全重放进
/// 续写请求（未配对的 function_call 会被上游拒绝）。这正是"经代理后模型
/// 称本回合没有 exec/文件工具"的根因之一：合法完成的工具调用轮，其
/// reasoning_tokens 恰好命中 518n-2 指纹（对不截断的上游约 1/step 概率）
/// 时被误判为截断，工具调用随续写丢失。类型缺失按阻断处理（宁可不续写，
/// 不丢数据）。message（含 commentary）沿用既有语义：允许续写并被丢弃，
/// 这是参考实现记录在案的取舍（见 codex_continue_e2e.rs）。
/// Prior art: router-for-me/CLIProxyAPI（MIT）#4048/#4219 系列对
/// "转换/续写路径丢失工具事件导致模型认为无工具"的同类根因修复。
fn buffered_item_blocks_continuation(item: &Value) -> bool {
    !matches!(item.get("type").and_then(Value::as_str), Some("message"))
}

fn sum_usage(acc: &mut Map<String, Value>, usage: Option<&Value>) {
    let Some(usage) = usage else {
        return;
    };

    for key in ["input_tokens", "output_tokens", "total_tokens"] {
        if let Some(v) = usage.get(key).and_then(Value::as_u64) {
            let cur = acc.get(key).and_then(Value::as_u64).unwrap_or(0);
            acc.insert(key.to_string(), json!(cur.saturating_add(v)));
        }
    }

    if let Some(v) = usage
        .get("input_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(Value::as_u64)
    {
        let entry = acc
            .entry("input_tokens_details".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(obj) = entry.as_object_mut() {
            let cur = obj
                .get("cached_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            obj.insert("cached_tokens".to_string(), json!(cur.saturating_add(v)));
        }
    }

    if let Some(v) = usage
        .get("output_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(Value::as_u64)
    {
        let entry = acc
            .entry("output_tokens_details".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(obj) = entry.as_object_mut() {
            let cur = obj
                .get("reasoning_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            obj.insert("reasoning_tokens".to_string(), json!(cur.saturating_add(v)));
        }
    }
}

#[derive(Default, Debug)]
struct FoldedUsage {
    proxy_billed_usage: Map<String, Value>,
    saw_usage: bool,
    first_input_tokens: Option<u64>,
    first_cached_tokens: Option<u64>,
    total_reasoning_tokens: u64,
    final_output_tokens: Option<u64>,
    final_reasoning_tokens: u64,
}

impl FoldedUsage {
    fn add_round_usage(&mut self, usage: Option<&Value>) {
        let Some(usage) = usage else {
            return;
        };

        sum_usage(&mut self.proxy_billed_usage, Some(usage));

        if !self.saw_usage {
            self.first_input_tokens = usage.get("input_tokens").and_then(Value::as_u64);
            self.first_cached_tokens = usage
                .get("input_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(Value::as_u64);
        }

        let round_reasoning = usage
            .get("output_tokens_details")
            .and_then(|d| d.get("reasoning_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        self.total_reasoning_tokens = self.total_reasoning_tokens.saturating_add(round_reasoning);
        self.final_output_tokens = usage.get("output_tokens").and_then(Value::as_u64);
        self.final_reasoning_tokens = round_reasoning;
        self.saw_usage = true;
    }

    fn public_usage(&self) -> Map<String, Value> {
        if !self.saw_usage {
            return Map::new();
        }

        let public_input = self.first_input_tokens.unwrap_or(0);
        let final_output = self
            .final_output_tokens
            .unwrap_or(self.final_reasoning_tokens);
        let final_visible_output = final_output.saturating_sub(self.final_reasoning_tokens);
        let public_output = self
            .total_reasoning_tokens
            .saturating_add(final_visible_output);

        let mut usage = Map::new();
        usage.insert("input_tokens".to_string(), json!(public_input));
        usage.insert("output_tokens".to_string(), json!(public_output));
        usage.insert(
            "total_tokens".to_string(),
            json!(public_input.saturating_add(public_output)),
        );

        if let Some(cached_tokens) = self.first_cached_tokens {
            usage.insert(
                "input_tokens_details".to_string(),
                json!({ "cached_tokens": cached_tokens }),
            );
        }

        usage.insert(
            "output_tokens_details".to_string(),
            json!({ "reasoning_tokens": self.total_reasoning_tokens }),
        );
        usage
    }
}

#[derive(Clone, Copy)]
struct MetadataUsage<'a> {
    public_usage: &'a Map<String, Value>,
    proxy_billed_usage: &'a Map<String, Value>,
    truncation_step: u64,
}

fn metadata_with_continue(
    mut response: Value,
    rounds: &[Value],
    stopped_reason: Option<&str>,
    usage: MetadataUsage<'_>,
    proxy_rounds: usize,
) -> Value {
    let Some(resp) = response.as_object_mut() else {
        return response;
    };

    let metadata = resp
        .entry("metadata".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !metadata.is_object() {
        *metadata = Value::Object(Map::new());
    }
    let md = metadata.as_object_mut().expect("metadata object");
    md.insert("proxy_rounds".to_string(), Value::Array(rounds.to_vec()));
    md.insert(
        "ccswitch_codex_continue".to_string(),
        json!({
            "enabled": true,
            "proxy_rounds": proxy_rounds,
            "stopped_reason": stopped_reason,
            "provider_failover_allowed": true,
            "continuation_via_forward_with_retry": true,
            "truncation_step": usage.truncation_step,
            "truncation_formula": "reasoning_tokens >= step - 2 && (reasoning_tokens + 2) % step == 0",
            "public_usage_formula": "first_round_input + all_round_reasoning + final_round_visible_output",
            "proxy_billed_usage": Value::Object(usage.proxy_billed_usage.clone()),
        }),
    );
    if !usage.public_usage.is_empty() {
        resp.insert(
            "usage".to_string(),
            Value::Object(usage.public_usage.clone()),
        );
    }
    response
}

struct TerminalReconstruction<'a> {
    base_response: Option<&'a Value>,
    final_output: &'a [Value],
    rounds: &'a [Value],
    stopped_reason: Option<&'a str>,
    usage: MetadataUsage<'a>,
    proxy_rounds: usize,
}

fn reconstruct_terminal(
    terminal: Option<Value>,
    reconstruction: TerminalReconstruction<'_>,
    seq: &mut u64,
) -> Value {
    let terminal_type = terminal
        .as_ref()
        .and_then(|ev| ev.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("response.incomplete")
        .to_string();

    let terminal_response = terminal
        .as_ref()
        .and_then(|ev| ev.get("response"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut response = reconstruction
        .base_response
        .cloned()
        .unwrap_or(terminal_response.clone());

    if let Some(resp) = response.as_object_mut() {
        if let Some(status) = terminal_response.get("status").cloned() {
            resp.insert("status".to_string(), status);
        } else if terminal_type == "response.incomplete" {
            resp.insert("status".to_string(), json!("incomplete"));
        }
        if let Some(details) = terminal_response.get("incomplete_details").cloned() {
            resp.insert("incomplete_details".to_string(), details);
        }
        // `response.created` normally carries `error: null`, while the final
        // `response.failed` contains the structured upstream failure. Since the
        // folded response is based on the created snapshot, copy the terminal
        // field explicitly or the client receives a failed event with no cause.
        if let Some(error) = terminal_response.get("error").cloned() {
            resp.insert("error".to_string(), error);
        }
        resp.insert(
            "output".to_string(),
            Value::Array(reconstruction.final_output.to_vec()),
        );
    }

    response = metadata_with_continue(
        response,
        reconstruction.rounds,
        reconstruction.stopped_reason,
        reconstruction.usage,
        reconstruction.proxy_rounds,
    );

    json!({
        "type": terminal_type,
        "response": response,
        "sequence_number": next_sequence(seq)
    })
}

fn synthetic_incomplete(
    base_response: Option<&Value>,
    final_output: &[Value],
    rounds: &[Value],
    reason: &str,
    usage: MetadataUsage<'_>,
    proxy_rounds: usize,
    seq: &mut u64,
) -> Value {
    let mut response = base_response.cloned().unwrap_or_else(|| json!({}));
    if let Some(resp) = response.as_object_mut() {
        resp.insert("status".to_string(), json!("incomplete"));
        resp.insert(
            "incomplete_details".to_string(),
            json!({
                "reason": reason,
            }),
        );
        resp.insert("output".to_string(), Value::Array(final_output.to_vec()));
    }
    response = metadata_with_continue(response, rounds, Some(reason), usage, proxy_rounds);

    json!({
        "type": "response.incomplete",
        "response": response,
        "sequence_number": next_sequence(seq)
    })
}

fn commentary_marker(marker: &str) -> Value {
    json!({
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "output_text",
            "text": marker,
        }],
        "phase": "commentary",
    })
}

fn input_as_vec(input: Option<&Value>) -> Vec<Value> {
    match input {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::Null) | None => Vec::new(),
        Some(other) => vec![other.clone()],
    }
}

fn merge_include(existing: Option<&Value>) -> Value {
    let mut out = Vec::<Value>::new();
    if let Some(Value::Array(items)) = existing {
        for item in items {
            if !out.iter().any(|v| v == item) {
                out.push(item.clone());
            }
        }
    }
    if !out.iter().any(|v| v.as_str() == Some(ENCRYPTED_INCLUDE)) {
        out.push(json!(ENCRYPTED_INCLUDE));
    }
    Value::Array(out)
}

pub(crate) fn prepare_initial_payload(base_body: &Value) -> Value {
    let mut body = base_body.clone();
    if !body.is_object() {
        body = json!({});
    }
    let Some(obj) = body.as_object_mut() else {
        return body;
    };

    obj.insert("include".to_string(), merge_include(obj.get("include")));
    body
}

pub(crate) fn build_continuation_payload(base_body: &Value, replay_tail: &[Value]) -> Value {
    let mut body = base_body.clone();
    if !body.is_object() {
        body = json!({});
    }
    let orig_input = input_as_vec(body.get("input"));
    let Some(obj) = body.as_object_mut() else {
        return body;
    };

    let mut input = orig_input;
    input.extend_from_slice(replay_tail);
    obj.insert("stream".to_string(), json!(true));
    obj.insert("input".to_string(), Value::Array(input));
    obj.insert("include".to_string(), merge_include(obj.get("include")));
    obj.remove("previous_response_id");
    body
}

struct BufferedItem {
    upstream_output_index: Value,
    events: Vec<Value>,
    item: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct CodexContinueRoundAttribution {
    pub(crate) round: usize,
    pub(crate) provider_id: String,
    pub(crate) outbound_model: Option<String>,
    pub(crate) response_model: Option<String>,
    pub(crate) usage: Option<Value>,
}

#[derive(Clone, Default)]
pub(crate) struct CodexContinueAttribution {
    rounds: std::sync::Arc<std::sync::Mutex<Vec<CodexContinueRoundAttribution>>>,
}

impl CodexContinueAttribution {
    fn record(&self, round: CodexContinueRoundAttribution) {
        self.rounds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(round);
    }

    pub(crate) fn snapshot(&self) -> Vec<CodexContinueRoundAttribution> {
        self.rounds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

pub(crate) struct FoldedProxyResponseArgs {
    pub(crate) first_response: ProxyResponse,
    pub(crate) first_connection_guard: Option<ActiveConnectionGuard>,
    pub(crate) forwarder: RequestForwarder,
    pub(crate) method: Method,
    pub(crate) endpoint: String,
    pub(crate) base_body: Value,
    pub(crate) headers: HeaderMap,
    pub(crate) extensions: Extensions,
    pub(crate) providers: Vec<Provider>,
    pub(crate) config: CodexContinueConfig,
    pub(crate) first_provider: Provider,
    pub(crate) first_outbound_model: Option<String>,
}

pub(crate) struct FoldedProxyResponse {
    pub(crate) response: ProxyResponse,
    pub(crate) attribution: CodexContinueAttribution,
}

struct FoldContinuationRequest {
    forwarder: RequestForwarder,
    method: Method,
    endpoint: String,
    base_body: Value,
    headers: HeaderMap,
    extensions: Extensions,
    providers: Vec<Provider>,
    config: CodexContinueConfig,
    first_provider: Provider,
    first_outbound_model: Option<String>,
    attribution: CodexContinueAttribution,
}

fn flush_buffered_item(
    item: BufferedItem,
    downstream_output_index: usize,
    seq: &mut u64,
) -> (Vec<Bytes>, Value) {
    let mut out = Vec::new();
    let mut final_item = item.item;
    for mut ev in item.events {
        set_output_index(&mut ev, downstream_output_index);
        set_sequence(&mut ev, seq);
        if event_type(&ev) == "response.output_item.done" {
            if let Some(done_item) = ev.get("item").cloned() {
                final_item = done_item;
            }
        }
        out.push(sse_event(&ev));
    }
    (out, final_item)
}

fn drain_buffered_items(
    buffered_items: &mut Vec<BufferedItem>,
    downstream_output_index: &mut usize,
    seq: &mut u64,
) -> (Vec<Bytes>, Vec<Value>) {
    let mut chunks = Vec::new();
    let mut final_items = Vec::new();
    for buffered in std::mem::take(buffered_items) {
        let (item_chunks, item) = flush_buffered_item(buffered, *downstream_output_index, seq);
        chunks.extend(item_chunks);
        *downstream_output_index += 1;
        final_items.push(item);
    }
    (chunks, final_items)
}

pub(crate) fn build_folded_proxy_response(args: FoldedProxyResponseArgs) -> FoldedProxyResponse {
    let status = args.first_response.status();
    let mut response_headers = args.first_response.headers().clone();
    response_headers.remove(CONTENT_LENGTH);
    response_headers.remove(CONTENT_ENCODING);
    response_headers.remove(TRANSFER_ENCODING);
    response_headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    let attribution = CodexContinueAttribution::default();
    let stream = fold_responses_stream(
        args.first_response,
        args.first_connection_guard,
        FoldContinuationRequest {
            forwarder: args.forwarder,
            method: args.method,
            endpoint: args.endpoint,
            base_body: args.base_body,
            headers: args.headers,
            extensions: args.extensions,
            providers: args.providers,
            config: args.config,
            first_provider: args.first_provider,
            first_outbound_model: args.first_outbound_model,
            attribution: attribution.clone(),
        },
    );
    FoldedProxyResponse {
        response: ProxyResponse::streamed(status, response_headers, stream),
        attribution,
    }
}

fn fold_responses_stream(
    first_response: ProxyResponse,
    first_connection_guard: Option<ActiveConnectionGuard>,
    continuation: FoldContinuationRequest,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut response = first_response;
        let mut connection_guard = first_connection_guard;
        let FoldContinuationRequest {
            forwarder,
            method,
            endpoint,
            base_body,
            headers,
            extensions,
            providers,
            config,
            first_provider,
            first_outbound_model,
            attribution,
        } = continuation;
        let namespace_restore_map =
            transform_codex_responses_namespace::namespace_restore_map(&base_body);
        let mut round_provider = first_provider;
        let mut round_outbound_model = first_outbound_model;
        let mut round_no = 0usize;
        let mut continuations = 0usize;
        let mut seq = 0u64;
        let mut downstream_output_index = 0usize;
        let mut base_response: Option<Value> = None;
        let mut final_output: Vec<Value> = Vec::new();
        let mut replay_tail: Vec<Value> = Vec::new();
        let mut rounds: Vec<Value> = Vec::new();
        let mut usage_acc = FoldedUsage::default();

        loop {
            round_no += 1;
            let mut parser = IncrementalSseParser::default();
            let stream = response.bytes_stream();
            tokio::pin!(stream);
            let _round_guard = connection_guard.take();
            let mut item_kind = Map::<String, Value>::new();
            let mut oi_map = Map::<String, Value>::new();
            let mut buffered_items: Vec<BufferedItem> = Vec::new();
            let mut round_reasoning: Vec<Value> = Vec::new();
            let mut terminal: Option<Value> = None;
            let mut stream_error: Option<String> = None;
            let mut saw_error_event = false;

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        stream_error = Some(e.to_string());
                        break;
                    }
                };
                for frame in parser.push(&chunk) {
                    match frame {
                        SseFrame::Done => {}
                        SseFrame::Event(mut ev) => {
                            if provider_needs_responses_namespace_flatten(&round_provider) {
                                transform_codex_responses_namespace::restore_sse_event_namespaces(
                                    &mut ev,
                                    &namespace_restore_map,
                                );
                            }
                            let t = event_type(&ev).to_string();
                            if t == "error" {
                                saw_error_event = true;
                            }
                            if t == "response.created" || t == "response.in_progress" {
                                if round_no == 1 {
                                    if t == "response.created" {
                                        base_response = ev.get("response").cloned();
                                    }
                                    set_sequence(&mut ev, &mut seq);
                                    yield Ok(sse_event(&ev));
                                }
                                continue;
                            }

                            if terminal_event(&ev) {
                                terminal = Some(ev);
                                continue;
                            }

                            if t == "response.output_item.added" {
                                let up_oi = output_index(&ev).unwrap_or_else(|| json!(format!("missing-{round_no}-{downstream_output_index}")));
                                let key = up_oi.to_string();
                                if output_item_type(&ev) == Some("reasoning") {
                                    item_kind.insert(key.clone(), json!("reasoning"));
                                    oi_map.insert(key, json!(downstream_output_index));
                                    set_output_index(&mut ev, downstream_output_index);
                                    downstream_output_index += 1;
                                    set_sequence(&mut ev, &mut seq);
                                    yield Ok(sse_event(&ev));
                                } else {
                                    item_kind.insert(key, json!("buffered"));
                                    let item = ev.get("item").cloned().unwrap_or_else(|| json!({}));
                                    buffered_items.push(BufferedItem {
                                        upstream_output_index: up_oi,
                                        events: vec![ev],
                                        item,
                                    });
                                }
                                continue;
                            }

                            let Some(up_oi) = output_index(&ev) else {
                                set_sequence(&mut ev, &mut seq);
                                yield Ok(sse_event(&ev));
                                continue;
                            };
                            let key = up_oi.to_string();
                            match item_kind.get(&key).and_then(Value::as_str) {
                                Some("reasoning") => {
                                    if let Some(ds_oi) = oi_map.get(&key).and_then(Value::as_u64) {
                                        set_output_index(&mut ev, ds_oi as usize);
                                    }
                                    if t == "response.output_item.done" {
                                        if let Some(item) = ev.get("item").cloned() {
                                            round_reasoning.push(item.clone());
                                            final_output.push(item);
                                        }
                                    }
                                    set_sequence(&mut ev, &mut seq);
                                    yield Ok(sse_event(&ev));
                                }
                                Some("buffered") => {
                                    if let Some(entry) = buffered_items.iter_mut().find(|entry| entry.upstream_output_index == up_oi) {
                                        if t == "response.output_item.done" {
                                            if let Some(item) = ev.get("item").cloned() {
                                                entry.item = item;
                                            }
                                        }
                                        entry.events.push(ev);
                                    }
                                }
                                _ => {
                                    set_sequence(&mut ev, &mut seq);
                                    yield Ok(sse_event(&ev));
                                }
                            }
                        }
                    }
                }
            }

            for frame in parser.finish() {
                match frame {
                    SseFrame::Done => {},
                    SseFrame::Event(mut ev) => {
                        if provider_needs_responses_namespace_flatten(&round_provider) {
                            transform_codex_responses_namespace::restore_sse_event_namespaces(
                                &mut ev,
                                &namespace_restore_map,
                            );
                        }
                        if event_type(&ev) == "error" {
                            saw_error_event = true;
                        }
                        if terminal_event(&ev) {
                            terminal = Some(ev);
                            continue;
                        }
                        set_sequence(&mut ev, &mut seq);
                        yield Ok(sse_event(&ev));
                    }
                }
            }

            let usage = terminal.as_ref().and_then(usage_from_terminal);
            let rt = reasoning_tokens(usage);
            usage_acc.add_round_usage(usage);
            if terminal.is_some() {
                attribution.record(CodexContinueRoundAttribution {
                    round: round_no,
                    provider_id: round_provider.id.clone(),
                    outbound_model: round_outbound_model.clone(),
                    response_model: terminal
                        .as_ref()
                        .and_then(|event| event.get("response"))
                        .and_then(|response| response.get("model"))
                        .and_then(Value::as_str)
                        .filter(|model| !model.is_empty())
                        .map(str::to_string),
                    usage: usage.cloned(),
                });
            }
            let has_encrypted = round_reasoning
                .last()
                .map(has_encrypted_content)
                .unwrap_or(false);
            let truncated = is_truncation_pattern(rt, config.step);
            let has_pending_tool_output = buffered_items
                .iter()
                .any(|entry| buffered_item_blocks_continuation(&entry.item));
            let can_continue = round_terminal_allows_continuation(
                terminal.as_ref(),
                saw_error_event,
            )
                && truncated
                && has_encrypted
                && !has_pending_tool_output
                && continuations < config.max_continuations;
            rounds.push(json!({
                "round": round_no,
                "reasoning_tokens": rt,
                "truncated": truncated,
                "has_encrypted_content": has_encrypted,
                "pending_tool_output": has_pending_tool_output,
                "provider_id": round_provider.id.clone(),
                "outbound_model": round_outbound_model.clone(),
                "error_event": saw_error_event,
                "continued": can_continue,
            }));

            if let Some(error) = stream_error {
                log::warn!("[CodexContinue] round {round_no} upstream stream error: {error}");
                let (chunks, items) = drain_buffered_items(
                    &mut buffered_items,
                    &mut downstream_output_index,
                    &mut seq,
                );
                for chunk in chunks {
                    yield Ok(chunk);
                }
                final_output.extend(items);
                let public_usage = usage_acc.public_usage();
                let metadata_usage = MetadataUsage {
                    public_usage: &public_usage,
                    proxy_billed_usage: &usage_acc.proxy_billed_usage,
                    truncation_step: config.step,
                };
                let ev = synthetic_incomplete(
                    base_response.as_ref(),
                    &final_output,
                    &rounds,
                    "upstream_error",
                    metadata_usage,
                    round_no,
                    &mut seq,
                );
                yield Ok(sse_event(&ev));
                yield Ok(sse_done());
                break;
            }

            if can_continue {
                continuations += 1;
                replay_tail.extend(round_reasoning.iter().cloned());
                replay_tail.push(commentary_marker(&config.marker));

                let next_body = build_continuation_payload(&base_body, &replay_tail);
                log::info!(
                    "[CodexContinue] round {round_no}: reasoning_tokens={:?}, continue {}/{}",
                    rt,
                    continuations,
                    config.max_continuations
                );

                match forwarder
                    .forward_with_retry(
                        &AppType::Codex,
                        method.clone(),
                        &endpoint,
                        next_body,
                        headers.clone(),
                        extensions.clone(),
                        providers.clone(),
                    )
                    .await
                {
                    Ok(mut result) => {
                        connection_guard = result.connection_guard.take();
                        response = result.response;
                        if !response.status().is_success() || !response.is_sse() {
                            let reason = if response.is_sse() { "upstream_status" } else { "upstream_not_sse" };
                            log::warn!(
                                "[CodexContinue] continuation round {} stopped: status={}, is_sse={}",
                                round_no + 1,
                                response.status().as_u16(),
                                response.is_sse()
                            );
                            let (chunks, items) = drain_buffered_items(
                                &mut buffered_items,
                                &mut downstream_output_index,
                                &mut seq,
                            );
                            for chunk in chunks {
                                yield Ok(chunk);
                            }
                            final_output.extend(items);
                            let public_usage = usage_acc.public_usage();
                            let metadata_usage = MetadataUsage {
                                public_usage: &public_usage,
                                proxy_billed_usage: &usage_acc.proxy_billed_usage,
                                truncation_step: config.step,
                            };
                            let ev = synthetic_incomplete(
                                base_response.as_ref(),
                                &final_output,
                                &rounds,
                                reason,
                                metadata_usage,
                                round_no,
                                &mut seq,
                            );
                            yield Ok(sse_event(&ev));
                            yield Ok(sse_done());
                            break;
                        }
                        round_provider = result.provider;
                        round_outbound_model = result.outbound_model;
                        continue;
                    }
                    Err(err) => {
                        log::warn!(
                            "[CodexContinue] continuation round {} forward failed: {}",
                            round_no + 1,
                            err.error
                        );
                        let (chunks, items) = drain_buffered_items(
                            &mut buffered_items,
                            &mut downstream_output_index,
                            &mut seq,
                        );
                        for chunk in chunks {
                            yield Ok(chunk);
                        }
                        final_output.extend(items);
                        let public_usage = usage_acc.public_usage();
                        let metadata_usage = MetadataUsage {
                            public_usage: &public_usage,
                            proxy_billed_usage: &usage_acc.proxy_billed_usage,
                            truncation_step: config.step,
                        };
                        let ev = synthetic_incomplete(
                            base_response.as_ref(),
                            &final_output,
                            &rounds,
                            "upstream_error",
                            metadata_usage,
                            round_no,
                            &mut seq,
                        );
                        yield Ok(sse_event(&ev));
                        yield Ok(sse_done());
                        break;
                    }
                }
            }

            let stopped_reason = if saw_error_event {
                Some("upstream_error_event")
            } else if terminal
                .as_ref()
                .is_some_and(|event| event_type(event) == "response.failed")
            {
                Some("response_failed")
            } else if truncated && !has_encrypted {
                Some("no_encrypted_content")
            } else if truncated && has_pending_tool_output {
                // 命中截断指纹但本轮已有待执行的工具调用：按完成响应下发，
                // 绝不吞掉工具调用去续写（见 buffered_item_blocks_continuation）。
                Some("pending_tool_output")
            } else if truncated && continuations >= config.max_continuations {
                Some("max_continue")
            } else if terminal.is_none() {
                Some("upstream_eof")
            } else {
                None
            };

            if terminal.is_none() {
                let (chunks, items) = drain_buffered_items(
                    &mut buffered_items,
                    &mut downstream_output_index,
                    &mut seq,
                );
                for chunk in chunks {
                    yield Ok(chunk);
                }
                final_output.extend(items);
                let public_usage = usage_acc.public_usage();
                let metadata_usage = MetadataUsage {
                    public_usage: &public_usage,
                    proxy_billed_usage: &usage_acc.proxy_billed_usage,
                    truncation_step: config.step,
                };
                let ev = synthetic_incomplete(
                    base_response.as_ref(),
                    &final_output,
                    &rounds,
                    stopped_reason.unwrap_or("upstream_eof"),
                    metadata_usage,
                    round_no,
                    &mut seq,
                );
                yield Ok(sse_event(&ev));
                yield Ok(sse_done());
                break;
            }

            let (chunks, items) = drain_buffered_items(
                &mut buffered_items,
                &mut downstream_output_index,
                &mut seq,
            );
            for chunk in chunks {
                yield Ok(chunk);
            }
            final_output.extend(items);

            let public_usage = usage_acc.public_usage();
            let metadata_usage = MetadataUsage {
                public_usage: &public_usage,
                proxy_billed_usage: &usage_acc.proxy_billed_usage,
                truncation_step: config.step,
            };
            let ev = reconstruct_terminal(
                terminal,
                TerminalReconstruction {
                    base_response: base_response.as_ref(),
                    final_output: &final_output,
                    rounds: &rounds,
                    stopped_reason,
                    usage: metadata_usage,
                    proxy_rounds: round_no,
                },
                &mut seq,
            );
            yield Ok(sse_event(&ev));
            yield Ok(sse_done());
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn truncation_formula_matches_518n_minus_2() {
        assert!(is_truncation_pattern(Some(516), 518));
        assert!(is_truncation_pattern(Some(1034), 518));
        assert!(!is_truncation_pattern(Some(515), 518));
        assert!(!is_truncation_pattern(Some(517), 518));
        assert!(!is_truncation_pattern(None, 518));
    }

    #[test]
    fn failed_or_cancelled_terminal_never_allows_continuation() {
        assert!(!terminal_allows_continuation(&json!({
            "type": "response.failed",
            "response": { "status": "failed" }
        })));
        assert!(!terminal_allows_continuation(&json!({
            "type": "response.incomplete",
            "response": { "status": "cancelled" }
        })));
        assert!(terminal_allows_continuation(&json!({
            "type": "response.incomplete",
            "response": { "status": "incomplete" }
        })));
        let completed = json!({
            "type": "response.completed",
            "response": { "status": "completed" }
        });
        assert!(round_terminal_allows_continuation(Some(&completed), false));
        assert!(
            !round_terminal_allows_continuation(Some(&completed), true),
            "an error SSE event in the round must veto continuation"
        );
    }

    #[test]
    fn reconstructed_failed_terminal_preserves_structured_error() {
        let base_response = json!({
            "id": "resp-1",
            "status": "in_progress",
            "error": null,
            "output": [],
        });
        let terminal = json!({
            "type": "response.failed",
            "response": {
                "id": "resp-1",
                "status": "failed",
                "error": {
                    "type": "server_error",
                    "code": "upstream_failed",
                    "message": "upstream exploded"
                }
            }
        });
        let public_usage = Map::new();
        let proxy_billed_usage = Map::new();
        let mut sequence = 0;

        let reconstructed = reconstruct_terminal(
            Some(terminal),
            TerminalReconstruction {
                base_response: Some(&base_response),
                final_output: &[],
                rounds: &[],
                stopped_reason: Some("response_failed"),
                usage: MetadataUsage {
                    public_usage: &public_usage,
                    proxy_billed_usage: &proxy_billed_usage,
                    truncation_step: DEFAULT_STEP,
                },
                proxy_rounds: 1,
            },
            &mut sequence,
        );

        assert_eq!(reconstructed["type"], "response.failed");
        assert_eq!(reconstructed["response"]["status"], "failed");
        assert_eq!(
            reconstructed["response"]["error"]["message"],
            "upstream exploded"
        );
        assert_eq!(
            reconstructed["response"]["error"]["code"],
            "upstream_failed"
        );
    }

    #[test]
    fn draining_buffered_assistant_item_preserves_events_and_final_output() {
        let mut buffered = vec![BufferedItem {
            upstream_output_index: json!(7),
            events: vec![
                json!({
                    "type": "response.output_item.added",
                    "output_index": 7,
                    "item": { "type": "message", "content": [] }
                }),
                json!({
                    "type": "response.output_item.done",
                    "output_index": 7,
                    "item": {
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": "partial answer" }]
                    }
                }),
            ],
            item: json!({ "type": "message", "content": [] }),
        }];
        let mut output_index = 2;
        let mut sequence = 10;

        let (chunks, items) = drain_buffered_items(&mut buffered, &mut output_index, &mut sequence);

        assert!(buffered.is_empty());
        assert_eq!(output_index, 3);
        assert_eq!(sequence, 12);
        assert_eq!(items[0]["content"][0]["text"], "partial answer");
        let emitted = String::from_utf8(
            chunks
                .into_iter()
                .flat_map(|chunk| chunk.to_vec())
                .collect(),
        )
        .expect("emitted SSE is UTF-8");
        assert!(emitted.contains("partial answer"));
        assert!(emitted.contains("\"output_index\":2"));
    }

    #[test]
    fn settings_normalization_caps_continuations() {
        let config = CodexContinueConfig {
            max_continuations: usize::MAX,
            step: 1,
            marker: "   ".to_string(),
            ..CodexContinueConfig::default()
        }
        .normalized();

        assert_eq!(config.max_continuations, MAX_CONTINUATIONS);
        assert_eq!(config.step, 3);
        assert_eq!(config.marker, DEFAULT_MARKER);
    }

    #[test]
    #[serial]
    fn environment_override_is_also_capped() {
        let previous = std::env::var_os("CCSWITCH_CODEX_CONTINUE_MAX");
        std::env::set_var("CCSWITCH_CODEX_CONTINUE_MAX", "999999");
        let config = CodexContinueConfig::from_settings_with_env(CodexContinueConfig::default());
        match previous {
            Some(value) => std::env::set_var("CCSWITCH_CODEX_CONTINUE_MAX", value),
            None => std::env::remove_var("CCSWITCH_CODEX_CONTINUE_MAX"),
        }

        assert_eq!(config.max_continuations, MAX_CONTINUATIONS);
    }

    #[test]
    fn only_message_items_allow_continuation() {
        // message（含 commentary）沿用既有"允许续写并丢弃"的取舍
        assert!(!buffered_item_blocks_continuation(&json!({
            "type": "message", "role": "assistant", "phase": "commentary"
        })));
        // 一切工具调用类输出项都必须阻断续写
        for kind in [
            "function_call",
            "custom_tool_call",
            "local_shell_call",
            "tool_search_call",
            "web_search_call",
            "mcp_call",
        ] {
            assert!(
                buffered_item_blocks_continuation(&json!({ "type": kind })),
                "{kind} must block continuation"
            );
        }
        // 类型缺失/非字符串：宁可不续写，不丢数据
        assert!(buffered_item_blocks_continuation(&json!({})));
        assert!(buffered_item_blocks_continuation(&json!({ "type": 42 })));
    }

    #[test]
    fn sse_parser_handles_split_chunks_multi_data_and_done() {
        let mut parser = IncrementalSseParser::default();
        let part1 = b"event: response.output_text.delta\ndata: {\"type\":\"response.output";
        let part2 = b"_text.delta\",\ndata: \"delta\":\"hi\"}\n\ndata: [DONE]\n\n";

        assert!(parser.push(part1).is_empty());
        let frames = parser.push(part2);
        assert_eq!(frames.len(), 2);
        match &frames[0] {
            SseFrame::Event(v) => {
                assert_eq!(v["type"], "response.output_text.delta");
                assert_eq!(v["delta"], "hi");
            }
            other => panic!("unexpected frame: {other:?}"),
        }
        assert_eq!(frames[1], SseFrame::Done);
    }

    #[test]
    fn folded_public_usage_avoids_replayed_input_and_keeps_billed_metadata() {
        let round1 = json!({
            "input_tokens": 100,
            "output_tokens": 526,
            "total_tokens": 626,
            "input_tokens_details": { "cached_tokens": 20 },
            "output_tokens_details": { "reasoning_tokens": 516 },
        });
        let round2 = json!({
            "input_tokens": 180,
            "output_tokens": 525,
            "total_tokens": 705,
            "input_tokens_details": { "cached_tokens": 30 },
            "output_tokens_details": { "reasoning_tokens": 516 },
        });
        let round3 = json!({
            "input_tokens": 240,
            "output_tokens": 80,
            "total_tokens": 320,
            "input_tokens_details": { "cached_tokens": 40 },
            "output_tokens_details": { "reasoning_tokens": 20 },
        });

        let mut usage = FoldedUsage::default();
        usage.add_round_usage(Some(&round1));
        usage.add_round_usage(Some(&round2));
        usage.add_round_usage(Some(&round3));

        let public_usage = usage.public_usage();
        assert_eq!(public_usage["input_tokens"], json!(100));
        assert_eq!(
            public_usage["input_tokens_details"]["cached_tokens"],
            json!(20)
        );
        assert_eq!(
            public_usage["output_tokens_details"]["reasoning_tokens"],
            json!(1052)
        );
        assert_eq!(public_usage["output_tokens"], json!(1112));
        assert_eq!(public_usage["total_tokens"], json!(1212));

        assert_eq!(usage.proxy_billed_usage["input_tokens"], json!(520));
        assert_eq!(usage.proxy_billed_usage["output_tokens"], json!(1131));
        assert_eq!(usage.proxy_billed_usage["total_tokens"], json!(1651));
        assert_eq!(
            usage.proxy_billed_usage["input_tokens_details"]["cached_tokens"],
            json!(90)
        );
        assert_eq!(
            usage.proxy_billed_usage["output_tokens_details"]["reasoning_tokens"],
            json!(1052)
        );

        let response = metadata_with_continue(
            json!({"id": "resp_1", "metadata": {}}),
            &[],
            None,
            MetadataUsage {
                public_usage: &public_usage,
                proxy_billed_usage: &usage.proxy_billed_usage,
                truncation_step: 518,
            },
            3,
        );

        assert_eq!(response["usage"], Value::Object(public_usage));
        let continue_md = &response["metadata"]["ccswitch_codex_continue"];
        assert_eq!(
            continue_md["proxy_billed_usage"]["input_tokens"],
            json!(520)
        );
        assert_eq!(continue_md["provider_failover_allowed"], json!(true));
        assert_eq!(
            continue_md["continuation_via_forward_with_retry"],
            json!(true)
        );
        assert_eq!(continue_md["truncation_step"], json!(518));
    }

    #[test]
    fn request_gating_requires_stream_and_reasoning_not_false() {
        let cfg = CodexContinueConfig {
            enabled: true,
            max_continuations: 8,
            step: 518,
            marker: DEFAULT_MARKER.to_string(),
        };
        assert!(should_enable_for_request(&json!({"stream": true}), &cfg));
        assert!(should_enable_for_request(
            &json!({"stream": true, "reasoning": {"effort": "high"}}),
            &cfg
        ));
        assert!(!should_enable_for_request(
            &json!({"stream": true, "reasoning": false}),
            &cfg
        ));
        assert!(!should_enable_for_request(&json!({"stream": false}), &cfg));
    }

    #[test]
    fn initial_payload_adds_encrypted_include_and_keeps_previous_response_id() {
        let base = json!({
            "model": "gpt-5",
            "stream": true,
            "previous_response_id": "resp_old",
            "input": [{"type": "message", "role": "user", "content": "hi"}],
        });

        let payload = prepare_initial_payload(&base);

        assert_eq!(payload["previous_response_id"], "resp_old");
        assert_eq!(payload["input"], base["input"]);
        let include = payload["include"].as_array().unwrap();
        assert!(include
            .iter()
            .any(|v| v.as_str() == Some(ENCRYPTED_INCLUDE)));
    }

    #[test]
    fn payload_builder_appends_replay_tail_and_preserves_encrypted_include() {
        let base = json!({
            "model": "gpt-5",
            "stream": false,
            "previous_response_id": "resp_old",
            "include": ["foo"],
            "input": [{"type": "message", "role": "user", "content": "hi"}],
        });
        let reasoning = json!({
            "type": "reasoning",
            "id": "rs_1",
            "encrypted_content": "secret",
        });
        let marker = commentary_marker("continue");
        let payload = build_continuation_payload(&base, &[reasoning.clone(), marker.clone()]);

        assert_eq!(payload["stream"], true);
        assert!(payload.get("previous_response_id").is_none());
        assert_eq!(payload["input"].as_array().unwrap().len(), 3);
        assert_eq!(payload["input"][1], reasoning);
        assert_eq!(payload["input"][2], marker);
        let include = payload["include"].as_array().unwrap();
        assert!(include.iter().any(|v| v.as_str() == Some("foo")));
        assert!(include
            .iter()
            .any(|v| v.as_str() == Some(ENCRYPTED_INCLUDE)));
    }

    #[test]
    fn payload_builder_preserves_multi_round_replay_tail_order() {
        let base = json!({
            "stream": true,
            "input": [{"role": "user", "content": "start"}],
        });
        let r1 = json!({"type": "reasoning", "id": "rs_1", "encrypted_content": "a"});
        let m1 = commentary_marker("continue 1");
        let r2 = json!({"type": "reasoning", "id": "rs_2", "encrypted_content": "b"});
        let m2 = commentary_marker("continue 2");

        let payload =
            build_continuation_payload(&base, &[r1.clone(), m1.clone(), r2.clone(), m2.clone()]);
        let input = payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 5);
        assert_eq!(input[1], r1);
        assert_eq!(input[2], m1);
        assert_eq!(input[3], r2);
        assert_eq!(input[4], m2);
    }
}

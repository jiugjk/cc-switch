# CodexCont reasoning continuation

CodexCont joins several truncated native Codex Responses streams into one client-visible stream. It is useful for reasoning-capable providers that stop at a repeatable internal reasoning-token boundary even though the task is not complete.

## Enable and configure

Open **Settings → Routing → CodexCont**. The default configuration is enabled, a maximum of 8 continuations, a token step of 518, and the marker:

> We need continue thinking. Do not summarize; continue from the previous reasoning state.

The maximum is a safety and cost limit, not a target. A normal completed response makes one upstream request. Every actual continuation makes another billable upstream request and can therefore increase latency and token usage.

Environment variables override the saved settings for the current process:

| Variable | Meaning |
| --- | --- |
| `CCSWITCH_CODEX_CONTINUE` | `true`/`false`, `1`/`0`, `on`/`off` |
| `CCSWITCH_CODEX_CONTINUE_MAX` | Maximum continuation count |
| `CCSWITCH_CODEX_CONTINUE_STEP` | Truncation fingerprint step; values below 3 are clamped to 3 |
| `CCSWITCH_CODEX_CONTINUE_MARKER` | Follow-up instruction; blank values are ignored |

Restart CC Switch after changing environment variables.

## When it runs

All of these gates must pass:

1. The request is a streaming `/v1/responses` request.
2. Reasoning is not explicitly disabled.
3. Routing remains native Responses end to end. Responses-to-Chat and Responses-to-Anthropic conversion paths are excluded.
4. The request is not a compact request.
5. The terminal usage reports a reasoning-token count matching the configured truncation fingerprint. With the default step, counts at `518 × n - 2` match.
6. The buffered output contains no tool/action item. `function_call`, `custom_tool_call`, `local_shell_call`, and unknown item types block continuation so an action can never be swallowed.
7. The configured maximum has not been reached.

When the gates pass, CC Switch carries forward the previous response output, requests `reasoning.encrypted_content`, appends the marker, and sends the follow-up through the same `RequestForwarder::forward_with_retry` path. Provider choice, retry, failover, headers, metrics, and quota attribution therefore remain consistent with ordinary requests.

## Stream folding and safety

The client receives one SSE stream. CodexCont renumbers sequence and output indexes across upstream segments, folds usage totals, emits one terminal response, and preserves both modern tool-call events and legacy streamed `function_call` events.

Continuation is deliberately conservative. A false negative produces a visibly incomplete answer that can be retried; a false positive around a tool call could hide an action the client must execute. For that reason, any non-message buffered output prevents continuation.

## Tuning

- Keep `step = 518` unless logs and upstream usage consistently demonstrate a different boundary.
- Reduce `maxContinuations` to control worst-case spend and latency. Increase it only after observing genuine repeated truncation.
- Keep the marker short and directive. Changing it affects the next upstream turn and can change model behavior.
- Disable CodexCont when comparing raw provider behavior. Requests still use normal CC Switch routing.

## Troubleshooting

If continuation does not occur, verify that proxy takeover is enabled for Codex, the selected provider uses native Responses, `stream` is true, reasoning is enabled, and the upstream terminal event includes `usage.output_tokens_details.reasoning_tokens`. A tool-producing turn intentionally does not continue.

If continuation occurs too often, restore the default step or disable the feature while collecting the terminal usage counts. If cost or latency is too high, lower the maximum. Route status and proxy logs should be checked before changing the fingerprint.

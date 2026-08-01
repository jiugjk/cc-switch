# CodexCont 推理自动续写

CodexCont 会把多段被截断的原生 Codex Responses 流拼接成客户端看到的一条完整流。它适合这样一类推理供应商：任务尚未完成，但每次都会在固定的内部 reasoning token 边界停止。

## 开启与配置

打开 **设置 → 路由 → CodexCont**。默认开启，最多续写 8 次，步长为 518，续写提示为：

> We need continue thinking. Do not summarize; continue from the previous reasoning state.

“最多 8 次”是费用与安全上限，不是目标次数。正常完成的回答只发送一次上游请求；每次真正发生续写，都会增加一次可计费的上游请求，也会增加延迟和 token 消耗。

以下环境变量会覆盖当前进程中保存的设置：

| 环境变量 | 含义 |
| --- | --- |
| `CCSWITCH_CODEX_CONTINUE` | `true`/`false`、`1`/`0`、`on`/`off` |
| `CCSWITCH_CODEX_CONTINUE_MAX` | 最大续写次数 |
| `CCSWITCH_CODEX_CONTINUE_STEP` | 截断指纹步长；小于 3 会被修正为 3 |
| `CCSWITCH_CODEX_CONTINUE_MARKER` | 续写指令；空值会被忽略 |

修改环境变量后需要重启 CC Switch。

## 触发条件

必须同时满足以下条件：

1. 请求是流式 `/v1/responses` 请求。
2. 请求没有明确关闭 reasoning。
3. 全程保持原生 Responses 协议；Responses→Chat 与 Responses→Anthropic 转换路径不会启用。
4. 不是 compact 请求。
5. 结束事件的 usage 中，reasoning token 数命中配置的截断指纹。默认步长下，`518 × n - 2` 会命中。
6. 已缓冲输出中没有任何工具或动作项。`function_call`、`custom_tool_call`、`local_shell_call` 以及未知类型都会阻止续写，避免吞掉客户端必须执行的动作。
7. 尚未达到最大续写次数。

条件满足后，CC Switch 会携带上一段响应输出，请求 `reasoning.encrypted_content`，追加续写提示，并继续走同一个 `RequestForwarder::forward_with_retry`。因此供应商选择、重试、故障转移、请求头、指标和额度归因都与普通请求一致。

## 流拼接与安全边界

客户端只看到一条 SSE 流。CodexCont 会跨上游分段重新编号 sequence 与 output index，合并 usage，只发送一个终止响应，并保留现代工具调用事件和旧版流式 `function_call` 事件。

这里刻意选择保守策略：漏续写只会留下一个可见的不完整回答，用户可以重试；但如果在工具调用附近误续写，就可能隐藏客户端必须执行的动作。因此只要出现非 message 输出项，本轮就不会自动续写。

## 调优建议

- 除非日志和上游 usage 持续证明边界不同，否则保留 `step = 518`。
- 降低 `maxContinuations` 可以限制最坏情况下的费用和延迟；只有确认存在多次真实截断后再提高。
- marker 应保持简短、明确。修改它会影响下一次上游推理行为。
- 对比供应商原始行为时可关闭 CodexCont；关闭后请求仍然正常经过 CC Switch 路由。

## 故障排查

没有发生续写时，检查 Codex 代理接管是否开启、供应商是否走原生 Responses、`stream` 是否为 true、reasoning 是否开启，以及结束事件是否包含 `usage.output_tokens_details.reasoning_tokens`。产生工具调用的轮次不续写是预期行为。

续写过于频繁时，先恢复默认步长，或暂时关闭功能并收集结束事件中的 usage 数值。费用或延迟过高时降低最大续写次数。修改截断指纹前，应先检查路由状态与代理日志。

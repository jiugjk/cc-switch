# 项目进度记录(progress.md)

任务:CC Switch × CodexCont 集成 + Windows 构建 + Debian 无头版
基线:farion1231/cc-switch v3.17.0(commit c8b0d60c),fork 到 jiugjk/cc-switch

## 阶段 0:仓库准备 ✅(2026-07-14)

- [x] `gh repo fork farion1231/cc-switch --clone=false` → https://github.com/jiugjk/cc-switch
- [x] `gh repo fork neteroster/CodexCont --clone=false` → https://github.com/jiugjk/CodexCont
- [x] fork 与上游同步(`gh repo sync`),clone 到本地 `C:\CCSwitch\cc-switch`,HEAD = c8b0d60c(v3.17.0 之后仅含 docs/i18n 提交)
- [x] 本机工具链确认:node v24.14.0 / pnpm 11.5.1 / rustc 1.94.0 / cargo 1.94.0
- [x] 创建本文件 progress.md

前期勘察(只读,位于 `C:\CCSwitch\.explore\`,与本 clone 同 commit,结论可直接使用):

- 代理转发/路由/重试/故障转移唯一入口:`src-tauri/src/proxy/forwarder.rs::RequestForwarder::forward_with_retry`
- `/v1/responses` 处理:`src-tauri/src/proxy/handlers.rs::handle_responses`
- Responses→Chat / Responses→Anthropic 转换判定:`src-tauri/src/proxy/providers/codex.rs::should_convert_codex_responses_to_{chat,anthropic}`
- 参考 fork(2836048681/cc-switch-codexcont,基线 3.16.5)的 `proxy/codex_continue.rs` 自包含、可近乎机械化移植到 3.17.0
- cc-switch-cli(SaladDay)提供 ratatui TUI/clap CLI/daemon 可借鉴,但其 proxy/database 为旧快照不可回移

## 阶段 A:CodexCont 集成 ✅(2026-07-14)

后端(Rust,自参考 fork 2836048681/cc-switch-codexcont 移植并适配 3.17.0):

- [x] 新增 `src-tauri/src/proxy/codex_continue.rs`(1206 行,自包含,含 7 个单测;截断指纹 518·n−2、SSE 折叠重写、续写 payload 构建、usage 累计)+ `proxy/mod.rs` 注册
- [x] `database/dao/settings.rs`:`get/set_codex_continue_config`(settings 键 `codex_continue_config`,复用现有 `get_setting/set_setting` JSON-blob 持久化体系;读取失败/缺失回退默认值,默认 enabled=true)
- [x] `commands/settings.rs`:`get/set_codex_continue_config` Tauri 命令(写入前归一化:`step ≥ 3`、空 marker 回退默认);`lib.rs` invoke_handler 注册
- [x] `proxy/handlers.rs::handle_responses` 接入:
  - 门控 = 双谓词整链扫描(候选链任一 Provider 命中 `should_convert_codex_responses_to_chat` **或** `should_convert_codex_responses_to_anthropic` 即禁用,比参考 fork 多覆盖 3.17.0 新增的 anthropic 转换)+ 请求级 `should_enable_for_request`(enabled && stream==true && reasoning≠false)
  - 折叠分支置于 anthropic/chat 两个转换分支之后、`process_response` 之前;续写轮复用 `forwarder.forward_with_retry(..., providers.clone())`(路由/重试/故障转移原样)
  - 新增 `build_codex_folded_stream_response` + `create_codex_folded_usage_collector`(折叠流 usage 记账,复用 `SseUsageCollector`/`create_logged_passthrough_stream`,签名与 3.16.5 逐字节一致已验证)
  - `/responses/compact` 不接入(与参考 fork 一致)
- [x] 环境变量覆盖保留:`CCSWITCH_CODEX_CONTINUE`(enabled)/`_MAX`/`_STEP`/`_MARKER`;配置加载失败、每轮续写、stopped_reason 均有日志

前端(React/TS):

- [x] `src/lib/api/settings.ts`:`CodexContinueConfig` 接口 + `get/setCodexContinueConfig`
- [x] 新增 `src/components/settings/CodexContinueConfigPanel.tsx`(开关乐观更新+失败回滚,数值/marker 草稿保存,同 Rectifier 面板模式)
- [x] `ProxyTabContent.tsx`:设置→路由(tabProxy)在 failover 与 rectifier 之间插入 `codexContinue` AccordionItem(BrainCircuit 图标)
- [x] i18n:`settings.advanced.codexContinue.*` 全 10 键补齐 en/zh/zh-TW/ja(参考 fork 仅有 defaultValue 缺陷已修复)
- [x] 未移植 fork 无关漂移(optimizer cacheTtl 等)

验证:

- [x] `cargo check --tests` 通过(仅上游既有警告)
- [x] `cargo test --lib codex_continue`:7/7 通过
- [x] `pnpm typecheck` 通过(注:本机 pnpm 11 会在 install 时往 pnpm-workspace.yaml 写入 `allowBuilds` 占位符并使脚本前置校验失败,已在本机全局设 `verify-deps-before-run=false` 并还原该文件,仓库不带此噪音)


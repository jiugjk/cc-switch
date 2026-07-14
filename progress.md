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

## 阶段 B:UI 修复 ✅(2026-07-14)

1. 顶部应用切换栏(验收 9/10):

- [x] 后端:`get_tool_versions` 新增 `include_latest`(默认 true)参数,false 时跳过 npm/github/pypi 最新版本网络查询,只做本地探测——供启动检测复用同一入口且零网络开销
- [x] 后端:`claude_desktop_config::is_installed()`(配置目录存在性轻量只读检查,Claude Desktop 无 CLI 可探测)+ 新命令 `is_claude_desktop_installed`(commands/provider.rs,lib.rs 已注册)
- [x] 前端 App.tsx:启动后台探测一次(`getToolVersions(…, includeLatest=false)` + `isClaudeDesktopInstalled`);`shownApps = visibleApps ∧ installed`;探测返回前按 visibleApps 原样显示避免闪烁,探测失败宁可多显示不误隐藏;activeApp 指向被隐藏应用时自动回退到第一个可见应用
- [x] 溢出:右侧动作按钮组本就 `shrink-0` + `useAutoCompact` 图标坍缩保留;配合未安装应用自动隐藏,非全屏窗口右侧不再被挤出
- [x] 测试:tests/msw/handlers.ts 补 `get_tool_versions`(空数组=不隐藏)与 `is_claude_desktop_installed` mock

2. 检测文案 i18n(验收 11):

- [x] 后端哨兵串 `not installed or not executable`(misc.rs)保持英文原样;AboutSection 展示层 `localizeToolError` 把哨兵映射为 `t("settings.toolNotInstalled")`(zh:未安装或不可执行 / zh-TW:未安裝或不可執行 / ja:未インストールまたは実行不可 / en 保原文),`[WSL:distro]` 前缀等诊断信息保留

3. 官网链接(验收 12):

- [x] AboutSection 新增 `TOOL_WEBSITES` 映射,工具名右侧渲染 ExternalLink 图标按钮(系统浏览器打开):Claude Code→claude.com/claude-code、Codex→developers.openai.com/codex、Gemini CLI→github.com/google-gemini/gemini-cli、OpenCode→opencode.ai、OpenClaw→openclaw.ai、Hermes→hermes-agent.nousresearch.com

4. 手动安装命令(验收 13):

- [x] `WINDOWS_ONE_CLICK_INSTALL_COMMANDS` 按任务原文替换(Claude irm 安装、Codex 微软商店/PowerShell 两法、Gemini pnpm、OpenCode/OpenClaw curl、Hermes iex);删除不再使用的 Hermes EncodedCommand 辅助常量;POSIX 侧不变

验证:

- [x] `cargo check` / `cargo fmt --check` 通过;`pnpm typecheck` / `format:check` 通过
- [x] `tests/integration/App.test.tsx` 单文件 4/4 通过
- [x] 全量 `pnpm test:unit`:443/446,其余 3 个失败为 **上游既有的本机并行 flaky**(已用纯净 c8b0d60c 基线工作树复现同样 3 个失败;单跑该文件全通过),非本次改动引入

## 阶段 D:GitHub Actions Windows 自动构建 ✅(2026-07-14,待推送后实测)

- [x] 新增 `.github/workflows/windows-build.yml`:`workflow_dispatch` + `push`(main)触发;windows-2022;Node 20 + pnpm 10.12.3(pnpm store 缓存)+ dtolnay/rust-toolchain@stable + Swatinem/rust-cache
- [x] 构建命令 `pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}' --bundles nsis,msi`——内联覆盖关闭 updater 签名产物(fork 无 TAURI_SIGNING_PRIVATE_KEY),绕开上游 release.yml 的密钥依赖
- [x] 产物上传:NSIS 安装包 / MSI 安装包 / 便携版 exe(actions/upload-artifact@v4,if-no-files-found: error)
- [x] 公共仓库 GitHub 托管 runner 免费;上游 ci.yml / release.yml 原样保留
- [ ] 推送后 `gh workflow run` + `gh run watch` 实测一次成功构建(阶段 E 推送后执行)

## 阶段 C:性能与内存优化(保守) ✅(2026-07-14)

方法:并行只读审计 workflow(3 个审计 agent:前端 bundle / Rust 热路径 / 内存增长)+ 每项发现独立对抗验证 agent;仅采纳验证通过的高置信改动。审计结论:所有长生命周期结构均已有界(无内存风险发现);1 项前端提案因前提错误被否决(相关依赖本就被急切锚定)。

采纳并落地 3 项:

- [x] **SettingsPage 懒加载**(src/App.tsx):`React.lazy` + 动态 import;SettingsPage 是 recharts(装机 7.4MB)+ victory-vendor + 11 个 d3-* 包的唯一引用链(经 UsageDashboard→UsageTrendChart),仅在用户显式打开设置时挂载。构建实测:主 chunk 拆出 564KB 的 SettingsPage chunk,recharts 已不在首屏 index chunk
- [x] **SessionManagerPage 懒加载**(src/App.tsx):同法;flexsearch + 1742 行会话管理子树拆出 112KB chunk,首屏不再包含
- [x] 两个懒组件共用 `<Suspense fallback={null}>` 边界,置于 currentView keyed、0.2s 渐入的 motion.div 内——chunk 加载空档被既有过渡动画掩盖,AnimatePresence 退出动画不受影响;Tauri 本地资源加载亚帧级
- [x] **Rust 热路径克隆消除**(proxy/handlers.rs handle_responses):`original_body/headers/extensions` 三个克隆改为仅在 `codex_continue_enabled` 时才执行(它们只被折叠分支读取)。CodexCont 不生效的每个 /v1/responses 请求(全局关闭/非流式/reasoning:false/候选链含转换 Provider)省去一次完整请求体 Value 克隆 + HeaderMap/Extensions 克隆;启用路径字节级不变
- [x] 不改 `[profile.release]`(opt-level="s"/thin LTO/codegen-units=1/strip 已是优化配置;panic=unwind 为 panic 钩子依赖,红线不动);vite 默认分包已足够,未加 manualChunks

验证:

- [x] `cargo check` / `cargo fmt --check` / `pnpm typecheck` 通过;`pnpm build:renderer` 成功且确认 recharts/flexsearch 均已移出首屏 chunk
- [x] `tests/integration/App.test.tsx` 单文件 4/4 通过(懒加载下 Suspense 行为正常);全量 444/446,失败仍为同一上游 flaky 文件

## 阶段 E:同步上游 + README + 推送(2026-07-14)

- [x] 用户在 GitHub 侧同步 fork(上游新增 2 commit:`9ca1a41f` 函数参数 type 归一化为 object、`6d316c0b` Chat→Responses 工具调用身份/顺序修复);两者仅触碰 `proxy/providers/{transform,streaming}_codex_chat.rs`,与本 fork 改动零文件重叠,且属于 CodexCont 门控主动排除的转换路径,功能正交
- [x] 本地 `git rebase origin/main` 干净完成(6 commit 无冲突重放)
- [x] rebase 后右量验证:`codex_continue` 7/7、上游新测 `streaming_codex_chat` 19/19 + `transform_codex_chat` 52/52 通过;`sqlite_home` 2 个失败确认为本机 `CODEX_API_KEY` + 真实 Codex 安装所致的环境噪音(单线程仍失败,非并行 flaky,基线同样失败,且位于未改动文件)
- [x] README.md / README_ZH.md 顶部新增"关于本 Fork"一节(CodexCont、切换栏、i18n、官网链接、安装命令、Windows CI 六项)




- [x] ���� fork main(9 commit,�� rebase ������� 2 commit);`gh workflow run` ʵ�� Windows Build:**�ɹ�**(18m49s,NSIS/MSI/��Я���������ϴ�,run 29335745521)�������� 14/15/16 ���

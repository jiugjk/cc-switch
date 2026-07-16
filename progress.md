# 项目进度记录(progress.md)

任务:CC Switch × CodexCont 集成 + Windows 构建 + 代理真实语义/能力/额度
基线:farion1231/cc-switch v3.17.0(commit c8b0d60c),fork 到 jiugjk/cc-switch

## 阶段 0:仓库准备 ✅(2026-07-14)

- [x] `gh repo fork farion1231/cc-switch --clone=false` → https://github.com/jiugjk/cc-switch
- [x] `gh repo fork neteroster/CodexCont --clone=false` → https://github.com/jiugjk/CodexCont
- [x] fork 与上游同步(`gh repo sync`),clone 到本地 `C:\CCSwitch\cc-switch`,HEAD = c8b0d60c(v3.17.0 之后仅含 docs/i18n 提交)
- [x] 本机工具链确认:node v24.14.0 / pnpm 11.5.1 / rustc 1.94.0 / cargo 1.94.0
- [x] 创建本文件 progress.md

前期勘察(只读,结论可直接使用):

- 代理转发/路由/重试/故障转移唯一入口:`src-tauri/src/proxy/forwarder.rs::RequestForwarder::forward_with_retry`
- `/v1/responses` 处理:`src-tauri/src/proxy/handlers.rs::handle_responses`
- Responses→Chat / Responses→Anthropic 转换判定:`src-tauri/src/proxy/providers/codex.rs::should_convert_codex_responses_to_{chat,anthropic}`
- 参考 fork(2836048681/cc-switch-codexcont,基线 3.16.5)的 `proxy/codex_continue.rs` 自包含、可近乎机械化移植到 3.17.0
- 第三方 CLI 参考仓(SaladDay/cc-switch-cli)的 clap/CLI 层可借鉴,但其 proxy/database 为旧快照不可回移

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




<<<<<<< HEAD
- [x] 推送 fork main(9 commit,含 rebase 后的上游 2 commit);`gh workflow run` 实测 Windows Build:**成功**(18m49s,NSIS/MSI/便携版三产物上传,run 29335745521)——验收 14/15/16 完成

## 阶段 G:上游同步 + 切换栏运行期重探测 + Codex 文案 + MS Store Codex 检测 + CodexCont 工具丢失修复 ✅\(2026-07-15)

### G1 上游同步(merge commit 573c92da)

- [x] 添加 upstream remote(farion1231/cc-switch),合并 upstream/main @ f6e37ed9(新增 2 commit:1cc52c7e SubRouter 赞助文档、f6e37ed9 CI 后端三平台矩阵 + Windows/macOS 平台门控测试修复)
- [x] ort 策略零冲突自动合并(重叠 4 文件:README/README_ZH/misc.rs/settings.rs);全量验证通过:typecheck / format:check / test:unit 444+2 已知 flaky(单文件重跑通过)/ cargo fmt / clippy -D warnings / cargo test(1 个 lib 测试因本机正在运行的 cc-switch.exe 占用 15721 端口跳过,基线同因,非回归)
- [x] 上游 f6e37ed9 顺带修复了 sqlite_home 两测试的 TOML 反斜杠转义问题,本机此前的环境噪音消失

### G2 切换栏「已安装」运行期重探测(修复:运行期间新装应用需重启才出现按钮)

- 根因:探测结果存于 App.tsx 内一次性 useEffect 的 useState,无任何再触发机制(无窗口焦点监听、非 React Query、托盘恢复不发前端事件、About 手动刷新不回写)
- [x] 新增 src/hooks/useInstalledApps.ts:探测迁入 React Query(key ["installed-apps"],staleTime 30s);Tauri getCurrentWindow().onFocusChanged(聚焦→refetchQueries stale:true,覆盖 alt-tab 切回)+ RQ 自带 visibilitychange 聚焦刷新(覆盖托盘恢复/最小化还原,lib.rs 托盘路径均 show+set_focus);并发探测由 RQ 去重,staleTime 节流反复聚焦;整次失败保留上次数据(宁可多显示);mergeShownApps 抽为纯函数
- [x] App.tsx 换用 hook;AboutSection 安装/升级动作完成后主动 invalidate(应用内安装窗口不失焦的场景)
- [x] 测试 tests/hooks/useInstalledApps.test.tsx:17 用例(merge 纯函数×4、probe 映射/隔离/fail-open、聚焦重探测出新按钮、新鲜期节流、blur 不触发、失败保留旧值、卸载清理)

### G3 Codex 端点文案修正(全 4 locale)

- 根因:providerForm.codexApiHint「填写兼容 OpenAI Response 格式的服务端点地址」①名称错(Responses API)②与高级选项「上游格式」三选项(Responses 直连 / Chat 需路由 / Anthropic Messages 需路由)矛盾——18 个内置 preset 本身就是 openai_chat
- [x] CodexFormFields 端点提示改为随 apiFormat 联动(仿 ClaudeFormFields 模式):Responses→codexApiHint(改写为「兼容 OpenAI Responses API…直连使用,不转换格式」),openai_chat→新键 codexApiHintChat,anthropic→新键 codexApiHintAnthropic(均注明「需开启路由接管,由本地路由转换为 Responses」)
- [x] codexConfig.advancedSectionHint 补上 Anthropic Messages 也需路由接管(原文只提 Chat Completions)
- [x] 删除 4 locale 中零引用且误导的死键 providerForm.codexApiFormat{Responses,OpenAIChat,Hint}(旧版描述完全缺 Anthropic)
- [x] 审查结论:upstreamFormat*/needsRouting 徽章/切换 toast(proxyReason*)/preset 文案均与 codex.rs 判定一致,未动

### G4 Microsoft Store 版 Codex 桌面应用检测(Windows)

- 调查(本机实测 + web 双源交叉):包身份 OpenAI.Codex(PFN OpenAI.Codex_2p2nqsd0c76g0,Store 产品 9PLM9XGG6VKS);2026-07 起显示名/主进程改为 ChatGPT 但包身份未变→绝不能按显示名匹配;同 publisher 另有 OpenAI.ChatGPT-Desktop 包→必须匹配名称段;无 AppExecutionAlias(PATH 上的 codex.exe 是应用落在 %LOCALAPPDATA%\Programs\OpenAI\Codexin 的独立 CLI);官方文档确认桌面应用与 CLI 共享真实 ~/.codex(MSIX 未虚拟化重定向,本机验证 LocalCache 无 .codex)→现有 Codex 配置管理零改动适用
- [x] 新增 src-tauri/src/codex_desktop.rs:HKCU AppModel Repository 包注册表枚举(winreg 现有依赖;本机实测 reg.exe /f 对该键假阴性,必须 API 枚举)‖ %LOCALAPPDATA%\Packages\OpenAI.Codex_* 目录存在;前缀 openai.codex_ 大小写不敏感匹配(get 防多字节越界);全只读、异常一律 false、非 Windows 恒 false;4 个单测(全名/家族名/大小写/同 publisher 排除/目录扫描),纯函数跨平台可测
- [x] 命令 is_codex_desktop_installed(commands/provider.rs,毗邻 is_claude_desktop_installed)+ lib.rs 注册 + settings.ts 包装 + probeInstalledApps 并入(codex = CLI∨桌面应用;桌面探测失败 catch(()=>false),不影响 CLI 信号)+ MSW handler + 7 个前端用例(仅 CLI/仅桌面/双装/双无/AppX 失败×2/焦点刷新)
- 限制:检测命中不代表 codex CLI 存在(About 环境检测仍只报告 CLI);CC-Switch 不读 CODEX_HOME(其自有 override-dir 设置为等价机制,维持现状)

### G5 CodexCont 审查 + 中转站工具丢失修复

- 全链路审查结论(含 8-agent 并行侦察 + CLIProxyAPI 对照):CodexCont 仅作用于 /v1/responses 原生直通链(整链任一转换型 Provider 即禁用);转换路径请求/响应侧为白名单复制,无 serde 静默丢字段,但存在显式丢弃点
- [x] **修复 1(根因,proxy/codex_continue.rs)**:续写轮的非 reasoning 缓冲项被整体丢弃——buffered_items 为轮内局部变量,can_continue 分支 continue 时既不下发、不进 final_output、也不重放。合法完成的工具调用轮 reasoning_tokens 恰好命中 518n− 指纹(≈1/518 概率,「偶尔」)即被误续写,function_call 被吞→Codex 表现为「本回合没有 exec/文件工具」。修复:缓冲项含 message 以外类型(function_call/custom_tool_call/local_shell_call/未知)即禁止续写,按完成响应正常下发;新增 stopped_reason=pending_tool_output 与 rounds[].pending_tool_output 诊断字段;message-only 轮维持参考实现既有取舍(允许续写,commentary 丢弃)。测试:新 e2e codex_continue_never_swallows_a_tool_call_round(新 fixture codex_tool_round.sse.txt:reasoning+function_call+指纹 usage→断言仅 1 次上游请求、call_id/name/arguments 逐字下发、metadata 可诊断)+ 单测 only_message_items_allow_continuation;既有 e2e×2 不回归
- [x] **修复 2(streaming_codex_chat.rs)**:旧版 delta.function_call 单调用流式形态(无 index/id,部分中转站仍用)此前被静默忽略→整个调用丢失。折算为固定 index 的 tool_calls 增量复用现有状态机,合成稳定 call_id;新测 converts_legacy_function_call_chat_sse_to_responses_sse。Prior art:router-for-me/CLIProxyAPI(MIT)#4219/commit 07455ecb(思路借鉴,Rust 重实现)
- [x] **修复 3(transform_codex_chat.rs)**:托管型工具类型(local_shell/web_search/file_search/mcp 等)在 Responses→Chat 转换中静默丢弃、且为唯一工具时连带移除 tool_choice/parallel_tool_calls——「模型称无工具」的另一直接机理但完全不可诊断。补 warn 日志(逐工具 + 全部丢空时);新测 responses_request_to_chat_drops_hosted_tool_types_entirely 锁定语义(混合场景 function 工具存活)
- CLIProxyAPI 对照结论:#4219(1a 缺 id 不发事件——上游 6d316c0b 已含等价修复+finalize 兜底,1d 参数先于身份——flush_ready+finalize 已覆盖)、#3298/#4251(namespace 扁平化——本仓已有 qualify+restore 实现与测试)、#4157(复杂 schema 简化——xAI 特定,未采纳)、#4048(全量重放——CodexCont 续写本就整体重放 input)。未移植代码,仅按根因对照排查
- 遗留风险(已记录,未修):Chat 同 index 换 id 重用会并串两调用;delta.content 数组形态被忽略(文本非工具);local_shell 桥接为 function 工具未实现(依赖 modelCatalog shell_type 强制,catalog 外模型经 Chat 转换仍会丢 shell);_ 前缀键全局过滤对依赖此类扩展字段的中转站不透传(设计使然)

### G6 验证与提交

- [x] 全量:pnpm typecheck/format:check/test:unit、cargo fmt --check/clippy -D warnings/cargo test(known-noise 跳过项见 G1)
- [x] 提交拆分:上游合并(573c92da)/ G2 / G3 / G4 / G5 各自独立 commit(hash 见最终报告)

## 阶段 H:文档审计 + 历史整理 + 遗留 bug 修复 ✅(2026-07-16)

### H0

- 代理真实语义 / 可观测状态 / 能力检测 / 额度归因(D1–D4)见阶段 I;实现向文档见 `C:\CCSwitch\docs\`。

### H1 文档审计(5-agent 并行只读核验)

- 逐条核验 progress.md 阶段 A–G 全部声明对得上真实代码/测试(附 file:line),无夸大。唯一小出入:G2 声称 17 个用例,实际 16 个 `it()`。
- 文档入口统一为 `C:\CCSwitch\docs\` + 短 `claude.md`;工作分支为 `main`。

### H2 审计发现的遗留 bug 修复(3 项,均带测试)

- [x] **Bug 1(medium,http_client.rs:319 `mask_url`)**:解析失败分支 `&url[..20]` 按字节切片,多字节字符(如中文代理地址)落在字节索引 20 中间时 panic(`byte index 20 is not a char boundary`);因本 fork `panic=unwind` 红线,会中断命令。改为按字符累加截断。新测 `test_mask_url_does_not_panic_on_multibyte_unparseable`
- [x] **Bug 2(medium,streaming_codex_chat.rs:154)**:Chat→Responses 流式只用 `delta.get("content").as_str()`,中转站以数组形态 `content:[{type:text,text:..}]` 下发时整段文本被静默丢弃→用户看到空/截断回答。新增 `chat_delta_content_text` 辅助:字符串直接用,数组拼接 `type∈{text,output_text}` 的 text(对齐非流式 `responses_content_to_chat_content` idiom),非文本部分打 warn。新测 `converts_array_form_content_delta_to_responses_text`
- [x] **Bug 3(low,streaming_codex_chat.rs:383)**:tool-call 状态仅按 Chat `index` 键入 BTreeMap,非规范上游在已 released 的 index 上复用不同 call_id 时两个调用会并串到一个 output item。保守修:id 分歧时打 warn 使可诊断,不改既有语义(规范上游不复用 index)

### H3 验证

- [x] `cargo test --lib proxy::http_client`:3/3 通过;`cargo test --lib streaming_codex_chat`:21/21 通过
- [x] `cargo fmt --check` / `cargo clippy -- -D warnings`:干净
- [x] 文档改动零代码影响;历史文件横幅均为 UTF-8 正确写入

## 阶段 I:代理真实语义可观测化 + capability 检测 + 故障转移额度归因 + 登录额度解耦（D1–D4）✅（2026-07-16；D5/I6 现场仍开放）

任务目标:不再以「Codex 配置页面中的 Base URL 是否变化」判断代理是否生效,而是完整确认并修正 Codex 运行时请求如何进入 CC Switch 代理、代理如何选择第三方上游、CodexCont 和路由功能在哪一层执行、关闭代理后哪些功能会失效;在此基础上完成登录额度解耦、quota 归因(不新开独立 fallback 开关)、协议能力适配与验证。实现向文档见 `C:\CCSwitch\docs\`(尤其 02/05/state)。

### I0 侦察修正(先确认真实语义,再改)

- 代理「接管」语义已核实:接管时改写 Codex live `config.toml` 的 `base_url` → `127.0.0.1:15721`、token 置 `PROXY_MANAGED` 占位、强制 `wire_api=responses`;原文件备份到 `proxy_live_backup`。**故 Codex 配置页 Base URL 不变 ≠ 代理未生效**——请求先落本地监听端口,再由转发层选上游。这正是 D1 要在 UI 说清的核心。
- 唯一出口 = `forwarder.rs::forward_with_retry` / `forward_with_retry_inner`:路由/重试/故障转移全在此;任何改动不得钉死某 Provider 或绕过该链。
- CodexCont 仅作用于 `/v1/responses` 原生直通链(任一故障转移候选需 Chat/Anthropic 转换即禁用)。三条 `/responses` 链:原生直通、Responses→Chat、Responses→Anthropic。
- AppSettings 持久化 = JSON 文件 `~/.cc-switch/settings.json`(`OnceLock<RwLock<AppSettings>>`),非 SQLite DAO;UI-only 开关搭现有 get/save_settings blob,无需新命令/lib.rs 注册。

### I1(D1)代理真实语义 + 可观测状态

- [x] `proxy/types.rs`:`ProxyStatus` 增 `last_route_protocol` / `last_masked_upstream` / `last_route_continuation` / `last_fallback_reason`(均 `#[serde(default)]`,ProxyStatus 手维护 snake_case 镜像)
- [x] `forwarder.rs` 成功点:调用 `resolve_capabilities` 解析线路协议/续写能力,`mask_url` 脱敏上游(仅留 scheme://host[:port],丢 path/query/userinfo),回写上述状态字段
- [x] 前端 `src/types/proxy.ts` 同步 4 字段;新组件 `ProxyStatusSummary.tsx`(只读,消费 `useProxyStatus`):展示本地监听地址 / 逻辑 Provider / 已经本地路由 / 线路协议 / 续写能力 / 脱敏上游 / 故障转移原因,并附「接管语义」说明(解释为何 Base URL 不变但代理仍生效)
- [x] `SettingsPage.tsx` General 页渲染;4 locale 加 `settings.proxyStatus.*`

### I2(D4)协议能力检测表

- [x] 新模块 `proxy/providers/capabilities.rs`:`CapabilityConfidence`(Heuristic<Declared<Probed<Confirmed)、`CapabilityState`(Supported/Unsupported/Unknown)、`ContinuationSupport`(Native/Degraded/Unsupported/Unknown)、`WireProtocol`(Responses/ChatCompletions/Anthropic/Native)、`ProviderCapabilitySnapshot`;`resolve_wire_protocol` / `resolve_capabilities`:声明能力(meta.capabilities)优先,否则由协议推导,否则 Unknown(fail-open);Chat/Anthropic → Degraded 续写(不等价原生 Responses)。仿 `model_capabilities.rs::resolve_image_input_capability` 模板
- [x] `provider.rs`:新增 `ProviderCapabilities`(全 `Option<bool>`,serde camelCase);`ProviderMeta` 加 `capabilities: Option<ProviderCapabilities>`
- [x] 5 个单测通过

### I3(D2/D3)故障转移额度归因 + 登录额度解耦(按用户反馈:不新增机制,优化既有故障转移;尽量适配其他应用)

- 复核:HTTP 429 已被 `categorize_proxy_error` 归为 Retryable → 额度耗尽本就走故障转移。故 D3 不新增开关,而是**精确归因**。
- [x] 新模块 `proxy/quota_error.rs`:`is_quota_exhaustion(&ProxyError) -> bool`——仅 UpstreamError 可能是额度;正文标记法(quota_exceeded/insufficient_quota/usage_limit_reached/credits_exhausted…);402 且正文不矛盾 = 额度;纯 rate_limit_exceeded 明确非额度;排除超时/DNS/TLS/auth/5xx/model-not-found/params。6 单测通过
- [x] `forwarder.rs` Retryable 分支:命中额度耗尽时写 `last_fallback_reason = quota_exhausted:<app>:<provider>` + FWD-QUOTA 日志;**语义不变**(仍经既有链故障转移)
- [x] (D2)`settings.rs`:`AppSettings` 加 `#[serde(default)] decouple_official_quota: bool` + Default + 访问器 `decouple_official_quota()`;`forward_with_retry_inner` 开头按开关过滤候选链——开启时丢弃 Codex 官方 Provider(纯函数 `filter_official_when_decoupled`,**永不清空链**:仅有官方渠道的用户保留其渠道)+ FWD-DECOUPLE 日志。3 单测通过
- [x] 前端:`types.ts` / `schemas/settings.ts` / `useSettingsForm.ts`(normalize + reset)加 `decoupleOfficialQuota`;新组件 `CustomApiQuotaSettings.tsx`(General 页 ToggleRow);4 locale 加 `settings.customApiQuota.*`

### I4 验证

- [x] 后端 `cargo test`:1990 通过;唯一 1 失败 = 本机运行中的 cc-switch.exe 占用 15721 端口(`update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active`,os error 10048),单独重跑通过 → 已知环境噪音,非回归(同 G1)
- [x] 后端新增 14 单测全通过(capabilities 5 + quota 6 + decouple 3)
- [x] 前端 `tsc --noEmit`:干净
- [x] 前端 `pnpm test:unit`:461 通过;2 失败 = 满载并行下 `App.test.tsx` 5000ms 超时(非断言失败),单文件重跑 4/4 通过 → 环境噪音,非回归
- [x] 4 locale 均为合法 JSON

### I5(D5)真实环境验证 — 用户执行(待补)

- [ ] 由用户跑最小真实请求(接管开/关、官方额度耗尽故障转移、自定义 API 解耦)并回填证据。清单:`C:\CCSwitch\docs\07-verify.md`

### I6 ChatGPT 桌面应用路由研究(用户要求逆向本机 ChatGPT,尽量走 cc-switch)— 静态侦察完成,待现场确认

**结论:分支 A(桌面应用遵从 `~/.codex/config.toml`,且 cc-switch 接管已经把其推理通道指向 `127.0.0.1:15721`)。路由本身无需新增任何工作。** 静态证据充分,尚未现场抓包确认。

- **为何是 A 而非 B/C**:
  - GUI 本身不做推理。Electron 壳(`app/ChatGPT.exe`,Chromium)拉起随包 Rust 引擎 `app\resources\codex.exe`(341MB,v0.144.4)以 `app-server` 模式运行;实测 spawn 参数为 `-c features.code_mode_host=true app-server --analytics-default-enabled`——唯一的 `-c` override 是 `code_mode_host`,Electron 层**没有任何 base_url override**(`app.asar` 中 `base_url` 出现次数 = 0)。
  - 该引擎从 `config.toml` 读 `base_url`(引擎二进制字符串引用:`config.toml` / `base_url` / `wire_api` / `model_provider` / `model_providers.custom`)。
  - 共享配置已确认:`~/.codex/config.toml` 含 `model_provider="custom"`、`[model_providers.custom] base_url="http://127.0.0.1:15721/v1"`、`wire_api="responses"`;同文件另有 `[desktop]` 与 `[tui]` 段 → GUI 与 CLI 共用同一文件,且 mtime 与 `auth.json` 一致(同一次接管改写)。
  - 非 C:有完整配置面,无需 MSIX 二进制打补丁;TLS pinning 与分支 A 无关(我们不做拦截,是应用自身按配置指向本地代理)。
  - 分支 B(系统/HTTPS_PROXY)可用但**不推荐**:引擎基于 reqwest 且识别 `HTTPS_PROXY/HTTP_PROXY/ALL_PROXY/NO_PROXY`,Chromium 识别系统代理——但正向代理会**连带捕获账号/登录/额度流量**(chatgpt.com/backend-api、auth.openai.com),既不该重定向也很可能破坏登录。config.toml 路由只干净地命中推理通道。
- **关键作用域限制(cc-switch 能/不能触及)**:
  - 模型 API 推理:**已经**经 cc-switch 路由 ✅
  - UI 登录门 + 服务端鉴权:**不受影响也无法影响**——走 chatgpt.com / auth.openai.com 的独立 Electron 原生通道,config.toml 不管辖(该通道 base 仅能由环境变量 `CODEX_API_BASE_URL` 覆盖,默认 chatgpt.com/backend-api)。应用**仍需有效 ChatGPT 登录**才能打开;绕过 = 伪造鉴权,受安全约束明确排除。
  - 官方额度:绑定账号通道,与代理无关;真正提供推理的是 cc-switch 的上游 Provider。
- **置信度/未知项(诚实标注)**:强**静态**证据,非现场确认。侦察为只读,未启动应用、未抓包,故**尚未观测到真实 `/v1/responses` 请求命中 127.0.0.1:15721**。升级为现场确认需:打开应用发一条消息,同时看 cc-switch 代理日志。另 `experimental_bearer_token` / `requires_openai_auth` 的**取值**按脱敏约束未读——其设置影响引擎附带 OpenAI auth 还是代理 auth。
- **关键证据路径**(WindowsApps 下 `OpenAI.Codex_26.707.9981.0_x64__2p2nqsd0c76g0`:`AppxManifest.xml`、`app\resources\codex.exe` 引擎、`app\resources\app.asar` Electron;`~/.codex/config.toml` 共享且已被 cc-switch 改写;`%LOCALAPPDATA%\Programs\OpenAI\Codex\bin\codex.exe` 独立 CLI)
- **建议**:路由视为已由分支 A 解决(与 codex CLI 相同机制,因共用 `~/.codex` 与同一引擎);不要加系统/正向代理(会过度捕获鉴权流量)。唯一开放项是经验性的:用户启动应用并在 cc-switch 日志确认请求落在 127.0.0.1:15721。并需对用户设定预期:应用的 ChatGPT 登录/额度 UI 不受 cc-switch 影响,可能显示与代理上游实际所服务的账号状态不一致。
- [ ] **待用户现场确认**:启动 ChatGPT 桌面应用发一条消息,观察 cc-switch 日志中出现命中 `127.0.0.1:15721` 的 `/v1/responses` 请求(与 D5 一并回填)

### I7 工作区文档收尾(2026-07-16)

- [x] 工作区实现向文档:`C:\CCSwitch\docs\`(00–07 + state + references);短入口 `claude.md`
- [x] 工作分支 **`main`**
- [x] Phase H2 + I 代码已提交于 `main`
=======
- [x] ���� fork main(9 commit,�� rebase ������� 2 commit);`gh workflow run` ʵ�� Windows Build:**�ɹ�**(18m49s,NSIS/MSI/��Я���������ϴ�,run 29335745521)�������� 14/15/16 ���
>>>>>>> origin/main

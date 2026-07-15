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




- [x] 推送 fork main(9 commit,含 rebase 后的上游 2 commit);`gh workflow run` 实测 Windows Build:**成功**(18m49s,NSIS/MSI/便携版三产物上传,run 29335745521)——验收 14/15/16 完成

## 阶段 F:Debian 无头版(分支 `debian-headless`)——架构勘察结论(2026-07-14)

三份并行只读勘察(命令面 / 参考 TUI 可复用性 / 前端 IPC 面)+ tauri 2.10 crate 机制核查,结论:

**命令面(288 个 Tauri 命令)**

- 261 个(91%)仅依赖 4 个托管 State(AppState / SkillServiceState / CopilotAuthState / CodexOAuthState),可直接 HTTP 化;全 codebase **零 `ipc::Channel`**。
- 仅 27 个非平凡:opener(6)、dialog(4)、updater(2)、store(2)、tray/window/lightweight(6)、桌面副作用(clipboard/terminal/auto-launch 等 6);全部可枚举、可 stub 或客户端替代。
- 需 WS 桥的事件仅 **12 个,纯单向 server→client 推送**(前端只 listen 从不 emit)。

**前端(React,零改动复用可行)**

- `invoke` 是唯一符号、294 处调用、**无中间包装层** → 一个 Vite alias(`@tauri-apps/api/core`)全覆盖;`listen` 同理。
- 仓库现成 `tests/msw/tauriMocks.ts` = 已验证的 HTTP invoke + 内存 listen shim,作浏览器 shim 模板。
- 需浏览器替代的原生流(10 个):文件/目录选择器、导入导出、开文件夹/终端、`open_external`、窗口控制条。

**Rust 无头化——头号风险与方案**

- tauri 2.10:Linux 上 `webkit2gtk` 是 `optional`(挂 `wry`),`gtk` 是硬依赖;目标"无 WebKitGTK"= 去 `wry`/webkit,保留 tauri+gtk 链接(不调 `gtk::init` 即可无 X 载入)。
- **裸 `AppHandle` = `AppHandle<Wry>`**:26 命令 + 4 处 service 存储点钉死 wry 运行时。
- 方案:引入 `EventSink` trait,桌面 impl 包 `AppHandle`、无头 impl 包 `broadcast::Sender`(→ WS);核心与运行时解耦。
- Cargo:`default = ["gui"]` 收纳 wry/tray/webkit/plugins(桌面构建字节级不变);`headless` = axum + argon2 + `tauri/test`,新增第二 `[[bin]]`。

**参考 cc-switch-cli 复用度**

- clap CLI + ratatui 渲染层 near-verbatim;数据/动作层(~17K LOC)需适配目标新签名(`AppState::new`、无 `state.config`、`add` 多参、`switch` 返回 `SwitchResult`、`AppType::ClaudeDesktop` 新臂)。
- **daemon supervisor(1860 LOC)丢弃**(绑死旧多 worker 代理模型);无头改用 axum + systemd `Restart=always`。

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

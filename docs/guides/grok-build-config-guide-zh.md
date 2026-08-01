# Grok Build 配置所有权

CC Switch 将 Grok Build 的实时 `config.toml` 分成两个所有权层，切换 API 供应商时不会再清空无关设置。

## 供应商模型资料与全局配置

Grok Build 供应商只拥有以下字段：

- `endpoints.models_base_url`
- `models.default` 与 `models.web_search`
- 完整的 `subagents` 段
- 全部 `model.*` 模型资料，包括凭据与自定义字段

其余内容都属于 Grok 安装级全局配置：MCP、遥测、harness 与 feature 开关、界面设置，以及未来新增的未知配置段。导入与实时回填只提取供应商层。切换到官方 OAuth 时只移除供应商层，全局层会继续保留。

Grok Build 供应商表单中的**应用模型资料到实时配置**会把当前草稿合并到实时文件；**编辑全局 config.toml**会打开完整文件，并显示最终解析出的实际路径。

## 文件路径解析

按以下优先级使用第一个命中的位置：

1. `GROK_CONFIG`：完整配置文件路径。
2. `GROK_HOME`：包含 `config.toml` 的目录。
3. CC Switch 设置中保存的 Grok 目录覆盖。
4. 默认的 `~/.grok/config.toml`。

相对环境变量路径会以 CC Switch 进程的工作目录为基准。建议使用绝对路径；修改环境变量后重启 CC Switch。

## 备份与隐私草稿

修改已有实时文件前，CC Switch 会把旧内容保存在本机 CC Switch 配置目录的 `grok-config-backups` 中。内容相同的写入不会重复备份，只保留最新 10 份。备份可能含有 API Key，应按敏感文件管理。

恢复备份前会先备份当前实时文件；删除只影响所选备份。

**添加隐私设置到草稿**会在编辑器草稿中关闭遥测、trace 上传与代码库上传。它不会自动落盘：请先检查具体 TOML，再明确点击保存。

## 代理接管

Grok Build 接管会改写每一个 `model.*`，而不只是 `models.default`。每个模型资料都会写入本地代理 URL、代理占位凭据和 `api_backend = "responses"`，同时更新 `endpoints.models_base_url`。模型名、env_key 声明、上下文窗口、subagent 选择和未知自定义键都会保留。这样默认模型、联网搜索模型与子代理模型都无法绕过代理。

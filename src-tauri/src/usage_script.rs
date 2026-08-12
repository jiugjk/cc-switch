use rquickjs::{Context, Function, Runtime};
use serde_json::Value;
use std::collections::HashMap;
use url::{Host, Url};

use crate::error::AppError;

/// 执行用量查询脚本
pub async fn execute_usage_script(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    timeout_secs: u64,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<Value, AppError> {
    // 检测是否为自定义模板模式
    // 优先使用前端传递的 template_type
    let is_custom_template = template_type.map(|t| t == "custom").unwrap_or(false);

    // 1. Bind template variables to private QuickJS globals.  Secret values
    // never become part of the source string (which can be echoed by a syntax
    // error or a diagnostic log).
    let bound_script = bind_template_vars(script_code);
    let known_secrets: Vec<String> = [
        Some(api_key),
        Some(base_url),
        access_token.filter(|value| !value.is_empty()),
        user_id.filter(|value| !value.is_empty()),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.is_empty())
    .map(ToOwned::to_owned)
    .collect();

    // 2. 验证 base_url 的安全性（仅当提供了 base_url 时）
    // 自定义模板模式下，用户可能不使用模板变量，而是直接在脚本中写完整 URL
    if should_validate_base_url(base_url, is_custom_template) {
        validate_base_url(base_url)?;
    }

    // 3. 在独立作用域中提取 request 配置（确保 Runtime/Context 在 await 前释放）
    // 用量脚本允许的最长执行时间（秒）。脚本来自不可信来源（deeplink、同步导入），
    // 必须限制其 CPU / 内存 / 栈占用，防止一个恶意/ buggy 脚本挂死整个后端。
    const USAGE_SCRIPT_TIMEOUT_SECS: u64 = 5;
    // 16 MiB 对仅构造 request 配置 / extractor 的脚本已经足够。
    const USAGE_SCRIPT_MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;

    /// 创建一个受控的 QuickJS Runtime：限制内存与栈，并安装执行时间中断器。
    fn create_script_runtime() -> Result<Runtime, AppError> {
        let runtime = Runtime::new().map_err(|e| {
            AppError::localized(
                "usage_script.runtime_create_failed",
                format!("创建 JS 运行时失败: {e}"),
                format!("Failed to create JS runtime: {e}"),
            )
        })?;

        // 内存和栈限制必须在 eval 前设置。
        runtime.set_memory_limit(USAGE_SCRIPT_MEMORY_LIMIT_BYTES);
        // set_max_stack_size 默认 256 KiB 够用，这里显式重申请求它保持一致。
        runtime.set_max_stack_size(256 * 1024);

        // 时间片中断器：每轮解释器循环检查是否超时，超时则抛出不可捕获的异常。
        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_secs(USAGE_SCRIPT_TIMEOUT_SECS))
            .ok_or_else(|| {
                AppError::localized(
                    "usage_script.invalid_timeout",
                    "无法计算脚本执行截止时间",
                    "Unable to compute script execution deadline",
                )
            })?;
        runtime.set_interrupt_handler(Some(Box::new(move || std::time::Instant::now() > deadline)));

        Ok(runtime)
    }

    let request_config = {
        let runtime = create_script_runtime()?;
        let context = Context::full(&runtime).map_err(|e| {
            AppError::localized(
                "usage_script.context_create_failed",
                format!(
                    "创建 JS 上下文失败: {}",
                    redact_script_error(&e, &known_secrets)
                ),
                format!(
                    "Failed to create JS context: {}",
                    redact_script_error(&e, &known_secrets)
                ),
            )
        })?;

        context.with(|ctx| {
            let globals = ctx.globals();
            globals.set("__ccsApiKey", api_key).map_err(|e| {
                AppError::localized(
                    "usage_script.variable_bind_failed",
                    format!(
                        "绑定 apiKey 失败: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                    format!(
                        "Failed to bind apiKey: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })?;
            globals.set("__ccsBaseUrl", base_url).map_err(|e| {
                AppError::localized(
                    "usage_script.variable_bind_failed",
                    format!(
                        "绑定 baseUrl 失败: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                    format!(
                        "Failed to bind baseUrl: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })?;
            globals
                .set("__ccsAccessToken", access_token.unwrap_or(""))
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.variable_bind_failed",
                        format!(
                            "绑定 accessToken 失败: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                        format!(
                            "Failed to bind accessToken: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                    )
                })?;
            globals
                .set("__ccsUserId", user_id.unwrap_or(""))
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.variable_bind_failed",
                        format!(
                            "绑定 userId 失败: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                        format!(
                            "Failed to bind userId: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                    )
                })?;

            // 执行用户代码，获取配置对象
            let config: rquickjs::Object = ctx.eval(bound_script.as_str()).map_err(|e| {
                AppError::localized(
                    "usage_script.config_parse_failed",
                    format!("解析配置失败: {}", redact_script_error(&e, &known_secrets)),
                    format!(
                        "Failed to parse config: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })?;

            // 提取 request 配置
            let request: rquickjs::Object = config.get("request").map_err(|e| {
                AppError::localized(
                    "usage_script.request_missing",
                    format!(
                        "缺少 request 配置: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                    format!(
                        "Missing request config: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })?;

            // 将 request 转换为 JSON 字符串
            let request_json: String = ctx
                .json_stringify(request)
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.request_serialize_failed",
                        format!(
                            "序列化 request 失败: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                        format!(
                            "Failed to serialize request: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                    )
                })?
                .ok_or_else(|| {
                    AppError::localized(
                        "usage_script.serialize_none",
                        "序列化返回 None",
                        "Serialization returned None",
                    )
                })?
                .get()
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.get_string_failed",
                        format!(
                            "获取字符串失败: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                        format!(
                            "Failed to get string: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                    )
                })?;

            Ok::<_, AppError>(request_json)
        })?
    }; // Runtime 和 Context 在这里被 drop

    // 4. 解析 request 配置
    let request: RequestConfig = serde_json::from_str(&request_config).map_err(|e| {
        AppError::localized(
            "usage_script.request_format_invalid",
            format!(
                "request 配置格式错误: {}",
                redact_script_error(&e, &known_secrets)
            ),
            format!(
                "Invalid request config format: {}",
                redact_script_error(&e, &known_secrets)
            ),
        )
    })?;

    // 5. 验证请求 URL（HTTPS 强制 + 同源检查）
    validate_request_url(&request.url, base_url, is_custom_template)?;

    // 6. 发送 HTTP 请求
    let response_data = send_http_request(&request, timeout_secs, &known_secrets).await?;

    // 7. 在独立作用域中执行 extractor（确保 Runtime/Context 在函数结束前释放）
    let result: Value = {
        let runtime = create_script_runtime()?;
        let context = Context::full(&runtime).map_err(|e| {
            AppError::localized(
                "usage_script.context_create_failed",
                format!(
                    "创建 JS 上下文失败: {}",
                    redact_script_error(&e, &known_secrets)
                ),
                format!(
                    "Failed to create JS context: {}",
                    redact_script_error(&e, &known_secrets)
                ),
            )
        })?;

        context.with(|ctx| {
            let globals = ctx.globals();
            globals.set("__ccsApiKey", api_key).map_err(|e| {
                AppError::localized(
                    "usage_script.variable_bind_failed",
                    format!(
                        "绑定 apiKey 失败: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                    format!(
                        "Failed to bind apiKey: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })?;
            globals.set("__ccsBaseUrl", base_url).map_err(|e| {
                AppError::localized(
                    "usage_script.variable_bind_failed",
                    format!(
                        "绑定 baseUrl 失败: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                    format!(
                        "Failed to bind baseUrl: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })?;
            globals
                .set("__ccsAccessToken", access_token.unwrap_or(""))
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.variable_bind_failed",
                        format!(
                            "绑定 accessToken 失败: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                        format!(
                            "Failed to bind accessToken: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                    )
                })?;
            globals
                .set("__ccsUserId", user_id.unwrap_or(""))
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.variable_bind_failed",
                        format!(
                            "绑定 userId 失败: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                        format!(
                            "Failed to bind userId: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                    )
                })?;

            // 重新 eval 获取配置对象
            let config: rquickjs::Object = ctx.eval(bound_script.as_str()).map_err(|e| {
                AppError::localized(
                    "usage_script.config_reparse_failed",
                    format!(
                        "重新解析配置失败: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                    format!(
                        "Failed to re-parse config: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })?;

            // 提取 extractor 函数
            let extractor: Function = config.get("extractor").map_err(|e| {
                AppError::localized(
                    "usage_script.extractor_missing",
                    format!(
                        "缺少 extractor 函数: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                    format!(
                        "Missing extractor function: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })?;

            // 将响应数据转换为 JS 值
            let response_js: rquickjs::Value =
                ctx.json_parse(response_data.as_str()).map_err(|e| {
                    AppError::localized(
                        "usage_script.response_parse_failed",
                        format!(
                            "解析响应 JSON 失败: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                        format!(
                            "Failed to parse response JSON: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                    )
                })?;

            // 调用 extractor(response)
            let result_js: rquickjs::Value = extractor.call((response_js,)).map_err(|e| {
                AppError::localized(
                    "usage_script.extractor_exec_failed",
                    format!(
                        "执行 extractor 失败: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                    format!(
                        "Failed to execute extractor: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })?;

            // 转换为 JSON 字符串
            let result_json: String = ctx
                .json_stringify(result_js)
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.result_serialize_failed",
                        format!(
                            "序列化结果失败: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                        format!(
                            "Failed to serialize result: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                    )
                })?
                .ok_or_else(|| {
                    AppError::localized(
                        "usage_script.serialize_none",
                        "序列化返回 None",
                        "Serialization returned None",
                    )
                })?
                .get()
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.get_string_failed",
                        format!(
                            "获取字符串失败: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                        format!(
                            "Failed to get string: {}",
                            redact_script_error(&e, &known_secrets)
                        ),
                    )
                })?;

            // 解析为 serde_json::Value
            serde_json::from_str(&result_json).map_err(|e| {
                AppError::localized(
                    "usage_script.json_parse_failed",
                    format!("JSON 解析失败: {}", redact_script_error(&e, &known_secrets)),
                    format!(
                        "JSON parse failed: {}",
                        redact_script_error(&e, &known_secrets)
                    ),
                )
            })
        })?
    }; // Runtime 和 Context 在这里被 drop

    // 8. 验证返回值格式
    validate_result(&result)?;

    Ok(result)
}

/// 请求配置结构
#[derive(Debug, serde::Deserialize)]
struct RequestConfig {
    url: String,
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
}

/// 发送 HTTP 请求
async fn send_http_request(
    config: &RequestConfig,
    timeout_secs: u64,
    known_secrets: &[String],
) -> Result<String, AppError> {
    // 使用全局 HTTP 客户端（已包含代理配置）
    let client = crate::proxy::http_client::get();
    // 约束超时范围，防止异常配置导致长时间阻塞（最小 2 秒，最大 30 秒）
    let request_timeout = std::time::Duration::from_secs(timeout_secs.clamp(2, 30));

    // 严格校验 HTTP 方法，非法值不回退为 GET
    let method: reqwest::Method = config.method.parse().map_err(|_| {
        AppError::localized(
            "usage_script.invalid_http_method",
            format!(
                "不支持的 HTTP 方法: {}",
                redact_script_error(&config.method, known_secrets)
            ),
            format!(
                "Unsupported HTTP method: {}",
                redact_script_error(&config.method, known_secrets)
            ),
        )
    })?;

    let mut req = client
        .request(method.clone(), &config.url)
        .timeout(request_timeout);

    // 添加请求头
    for (k, v) in &config.headers {
        req = req.header(k, v);
    }

    // 添加请求体
    if let Some(body) = &config.body {
        req = req.body(body.clone());
    }

    // 发送请求
    let resp = req.send().await.map_err(|e| {
        AppError::localized(
            "usage_script.request_failed",
            format!("请求失败: {}", redact_script_error(&e, known_secrets)),
            format!("Request failed: {}", redact_script_error(&e, known_secrets)),
        )
    })?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| {
        AppError::localized(
            "usage_script.read_response_failed",
            format!("读取响应失败: {}", redact_script_error(&e, known_secrets)),
            format!(
                "Failed to read response: {}",
                redact_script_error(&e, known_secrets)
            ),
        )
    })?;

    if !status.is_success() {
        let preview = if text.len() > 200 {
            let mut safe_cut = 200usize;
            while !text.is_char_boundary(safe_cut) {
                safe_cut = safe_cut.saturating_sub(1);
            }
            format!("{}...", &text[..safe_cut])
        } else {
            text.clone()
        };
        let preview = redact_script_error(&preview, known_secrets);
        return Err(AppError::localized(
            "usage_script.http_error",
            format!("HTTP {status} : {preview}"),
            format!("HTTP {status} : {preview}"),
        ));
    }

    Ok(text)
}

/// 验证脚本返回值（支持单对象或数组）
fn validate_result(result: &Value) -> Result<(), AppError> {
    // 如果是数组，验证每个元素
    if let Some(arr) = result.as_array() {
        if arr.is_empty() {
            return Err(AppError::localized(
                "usage_script.empty_array",
                "脚本返回的数组不能为空",
                "Script returned empty array",
            ));
        }
        for (idx, item) in arr.iter().enumerate() {
            validate_single_usage(item).map_err(|e| {
                AppError::localized(
                    "usage_script.array_validation_failed",
                    format!("数组索引[{idx}]验证失败: {e}"),
                    format!("Validation failed at index [{idx}]: {e}"),
                )
            })?;
        }
        return Ok(());
    }

    // 如果是单对象，直接验证（向后兼容）
    validate_single_usage(result)
}

/// 验证单个用量数据对象
fn validate_single_usage(result: &Value) -> Result<(), AppError> {
    let obj = result.as_object().ok_or_else(|| {
        AppError::localized(
            "usage_script.must_return_object",
            "脚本必须返回对象或对象数组",
            "Script must return object or array of objects",
        )
    })?;

    // 所有字段均为可选，只进行类型检查
    if obj.contains_key("isValid")
        && !result["isValid"].is_null()
        && !result["isValid"].is_boolean()
    {
        return Err(AppError::localized(
            "usage_script.isvalid_type_error",
            "isValid 必须是布尔值或 null",
            "isValid must be boolean or null",
        ));
    }
    if obj.contains_key("invalidMessage")
        && !result["invalidMessage"].is_null()
        && !result["invalidMessage"].is_string()
    {
        return Err(AppError::localized(
            "usage_script.invalidmessage_type_error",
            "invalidMessage 必须是字符串或 null",
            "invalidMessage must be string or null",
        ));
    }
    if obj.contains_key("remaining")
        && !result["remaining"].is_null()
        && !result["remaining"].is_number()
    {
        return Err(AppError::localized(
            "usage_script.remaining_type_error",
            "remaining 必须是数字或 null",
            "remaining must be number or null",
        ));
    }
    if obj.contains_key("unit") && !result["unit"].is_null() && !result["unit"].is_string() {
        return Err(AppError::localized(
            "usage_script.unit_type_error",
            "unit 必须是字符串或 null",
            "unit must be string or null",
        ));
    }
    if obj.contains_key("total") && !result["total"].is_null() && !result["total"].is_number() {
        return Err(AppError::localized(
            "usage_script.total_type_error",
            "total 必须是数字或 null",
            "total must be number or null",
        ));
    }
    if obj.contains_key("used") && !result["used"].is_null() && !result["used"].is_number() {
        return Err(AppError::localized(
            "usage_script.used_type_error",
            "used 必须是数字或 null",
            "used must be number or null",
        ));
    }
    if obj.contains_key("planName")
        && !result["planName"].is_null()
        && !result["planName"].is_string()
    {
        return Err(AppError::localized(
            "usage_script.planname_type_error",
            "planName 必须是字符串或 null",
            "planName must be string or null",
        ));
    }
    if obj.contains_key("extra") && !result["extra"].is_null() && !result["extra"].is_string() {
        return Err(AppError::localized(
            "usage_script.extra_type_error",
            "extra 必须是字符串或 null",
            "extra must be string or null",
        ));
    }

    Ok(())
}

/// 构建替换变量后的脚本，保持与旧版脚本的兼容性
fn redact_script_error(error: impl std::fmt::Display, known_secrets: &[String]) -> String {
    crate::redact_known_secrets(&error.to_string(), known_secrets)
}

fn template_identifier(name: &str) -> Option<&'static str> {
    match name {
        "apiKey" => Some("__ccsApiKey"),
        "baseUrl" => Some("__ccsBaseUrl"),
        "accessToken" => Some("__ccsAccessToken"),
        "userId" => Some("__ccsUserId"),
        _ => None,
    }
}

/// Replace placeholders with references to QuickJS globals without inserting
/// their values into source text.  Placeholders in quoted strings become a
/// concatenation expression (`"Bearer " + __ccsApiKey`); bare placeholders
/// become the identifier directly.  Template literals use `${...}`.
fn bind_template_vars(script_code: &str) -> String {
    const OPEN: [char; 3] = ['\'', '"', '`'];
    let chars: Vec<char> = script_code.chars().collect();
    let mut output = String::with_capacity(script_code.len() + 32);
    let mut index = 0usize;

    while index < chars.len() {
        let current = chars[index];
        if OPEN.contains(&current) {
            let quote = current;
            let start = index;
            index += 1;
            let mut escaped = false;
            let mut placeholder_found = false;
            let mut segments: Vec<String> = Vec::new();
            let mut identifiers: Vec<&'static str> = Vec::new();
            let mut segment_start = index;

            while index < chars.len() {
                let character = chars[index];
                if !escaped && character == quote {
                    break;
                }
                if !escaped
                    && character == '{'
                    && index + 1 < chars.len()
                    && chars[index + 1] == '{'
                {
                    if let Some(end) = chars[index + 2..]
                        .windows(2)
                        .position(|pair| pair == ['}', '}'])
                    {
                        let name_end = index + 2 + end;
                        let name: String = chars[index + 2..name_end].iter().collect();
                        if let Some(identifier) = template_identifier(name.trim()) {
                            placeholder_found = true;
                            segments.push(chars[segment_start..index].iter().collect());
                            identifiers.push(identifier);
                            index = name_end + 2;
                            segment_start = index;
                            escaped = false;
                            continue;
                        }
                    }
                }
                escaped = !escaped && character == '\\';
                index += 1;
            }

            if index >= chars.len() {
                // Unterminated strings are left untouched so QuickJS reports
                // the original syntax location.
                output.extend(chars[start..].iter());
                break;
            }

            if !placeholder_found {
                output.extend(chars[start..=index].iter());
            } else if quote == '`' {
                // A template literal can safely interpolate globals directly.
                let mut raw: String = chars[start + 1..index].iter().collect();
                for identifier in identifiers {
                    let marker = match identifier {
                        "__ccsApiKey" => "apiKey",
                        "__ccsBaseUrl" => "baseUrl",
                        "__ccsAccessToken" => "accessToken",
                        "__ccsUserId" => "userId",
                        _ => unreachable!(),
                    };
                    raw = raw.replacen(
                        &format!("{{{{{marker}}}}}"),
                        &format!("${{{identifier}}}"),
                        1,
                    );
                }
                output.push('`');
                output.push_str(&raw);
                output.push('`');
            } else {
                output.push('(');
                for (position, segment) in segments.iter().enumerate() {
                    if !segment.is_empty() {
                        if position > 0 {
                            output.push_str(" + ");
                        }
                        output.push(quote);
                        output.push_str(segment);
                        output.push(quote);
                    }
                    if position < identifiers.len() {
                        if position > 0 || !segment.is_empty() {
                            output.push_str(" + ");
                        }
                        output.push_str(identifiers[position]);
                    }
                }
                let tail: String = chars[segment_start..index].iter().collect();
                if !tail.is_empty() {
                    if !identifiers.is_empty() {
                        output.push_str(" + ");
                    }
                    output.push(quote);
                    output.push_str(&tail);
                    output.push(quote);
                }
                output.push(')');
            }
            index += 1;
            continue;
        }

        if current == '{' && index + 1 < chars.len() && chars[index + 1] == '{' {
            if let Some(end) = chars[index + 2..]
                .windows(2)
                .position(|pair| pair == ['}', '}'])
            {
                let name_end = index + 2 + end;
                let name: String = chars[index + 2..name_end].iter().collect();
                if let Some(identifier) = template_identifier(name.trim()) {
                    output.push_str(identifier);
                    index = name_end + 2;
                    continue;
                }
            }
        }

        output.push(current);
        index += 1;
    }

    output
}

/// 验证 base_url 的基本安全性
fn validate_base_url(base_url: &str) -> Result<(), AppError> {
    if base_url.is_empty() {
        return Err(AppError::localized(
            "usage_script.base_url_empty",
            "base_url 不能为空",
            "base_url cannot be empty",
        ));
    }

    // 解析 URL
    let parsed_url = Url::parse(base_url).map_err(|e| {
        AppError::localized(
            "usage_script.base_url_invalid",
            format!("无效的 base_url: {e}"),
            format!("Invalid base_url: {e}"),
        )
    })?;

    let is_loopback = is_loopback_host(&parsed_url);

    // 必须是 HTTPS（允许 localhost 用于开发）
    if parsed_url.scheme() != "https" && !is_loopback {
        return Err(AppError::localized(
            "usage_script.base_url_https_required",
            "base_url 必须使用 HTTPS 协议（localhost 除外）",
            "base_url must use HTTPS (localhost allowed)",
        ));
    }

    // 检查主机名格式有效性
    let hostname = parsed_url.host_str().ok_or_else(|| {
        AppError::localized(
            "usage_script.base_url_hostname_missing",
            "base_url 必须包含有效的主机名",
            "base_url must include a valid hostname",
        )
    })?;

    // 基本的主机名格式检查
    if hostname.is_empty() {
        return Err(AppError::localized(
            "usage_script.base_url_hostname_empty",
            "base_url 主机名不能为空",
            "base_url hostname cannot be empty",
        ));
    }

    Ok(())
}

fn should_validate_base_url(base_url: &str, is_custom_template: bool) -> bool {
    !base_url.is_empty() && !is_custom_template
}

/// 验证请求 URL 是否安全（HTTPS 强制 + 同源检查）
fn validate_request_url(
    request_url: &str,
    base_url: &str,
    is_custom_template: bool,
) -> Result<(), AppError> {
    // 解析请求 URL
    let parsed_request = Url::parse(request_url).map_err(|e| {
        AppError::localized(
            "usage_script.request_url_invalid",
            format!("无效的请求 URL: {e}"),
            format!("Invalid request URL: {e}"),
        )
    })?;

    let is_request_loopback = is_loopback_host(&parsed_request);

    // 必须使用 HTTPS（允许 localhost 用于开发）
    // 自定义模板模式下，允许用户自行决定是否使用 HTTP（用户需自行承担安全风险）
    if !is_custom_template && parsed_request.scheme() != "https" && !is_request_loopback {
        return Err(AppError::localized(
            "usage_script.request_https_required",
            "请求 URL 必须使用 HTTPS 协议（localhost 除外）",
            "Request URL must use HTTPS (localhost allowed)",
        ));
    }

    // 如果提供了 base_url（非空），则进行同源检查
    // 🔧 自定义模板模式下，用户可以自由访问任意 HTTPS 域名，跳过同源检查
    if !base_url.is_empty() && !is_custom_template {
        // 解析 base URL
        let parsed_base = Url::parse(base_url).map_err(|e| {
            AppError::localized(
                "usage_script.base_url_invalid",
                format!("无效的 base_url: {e}"),
                format!("Invalid base_url: {e}"),
            )
        })?;

        // 核心安全检查：必须与 base_url 同源（相同域名和端口）
        if parsed_request.host_str() != parsed_base.host_str() {
            return Err(AppError::localized(
                "usage_script.request_host_mismatch",
                format!(
                    "请求域名 {} 与 base_url 域名 {} 不匹配（必须是同源请求）",
                    parsed_request.host_str().unwrap_or("unknown"),
                    parsed_base.host_str().unwrap_or("unknown")
                ),
                format!(
                    "Request host {} must match base_url host {} (same-origin required)",
                    parsed_request.host_str().unwrap_or("unknown"),
                    parsed_base.host_str().unwrap_or("unknown")
                ),
            ));
        }

        // 检查端口是否匹配（考虑默认端口）
        // 使用 port_or_known_default() 会自动处理默认端口（http->80, https->443）
        match (
            parsed_request.port_or_known_default(),
            parsed_base.port_or_known_default(),
        ) {
            (Some(request_port), Some(base_port)) if request_port == base_port => {
                // 端口匹配，继续执行
            }
            (Some(request_port), Some(base_port)) => {
                return Err(AppError::localized(
                    "usage_script.request_port_mismatch",
                    format!("请求端口 {request_port} 必须与 base_url 端口 {base_port} 匹配"),
                    format!("Request port {request_port} must match base_url port {base_port}"),
                ));
            }
            _ => {
                // 理论上不会发生，因为 port_or_known_default() 应该总是返回 Some
                return Err(AppError::localized(
                    "usage_script.request_port_unknown",
                    "无法确定端口号",
                    "Unable to determine port number",
                ));
            }
        }
    }

    Ok(())
}

/// 判断 URL 是否指向本机（localhost / loopback）
fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn test_https_bypass_prevention() {
        // 非本地域名的 HTTP 应该被拒绝
        let result = validate_base_url("http://127.0.0.1.evil.com/api");
        assert!(
            result.is_err(),
            "Should reject HTTP for non-localhost domains"
        );
    }

    #[test]
    fn test_custom_template_allows_http_lan_request_with_different_base_url() {
        assert!(
            !should_validate_base_url("http://10.37.192.156:8090/anthropic", true),
            "Custom scripts should not validate an unused provider base_url fallback"
        );

        let result = validate_request_url(
            "http://10.37.192.156:18344/user/balance",
            "http://10.37.192.156:8090/anthropic",
            true,
        );
        assert!(
            result.is_ok(),
            "Custom usage scripts should be able to call an explicit HTTP quota endpoint"
        );
    }

    #[test]
    fn test_port_comparison() {
        // 测试端口比较逻辑是否正确处理默认端口和显式端口

        // 测试用例：(base_url, request_url, should_match)
        let test_cases = vec![
            // HTTPS默认端口测试
            (
                "https://api.example.com",
                "https://api.example.com/v1/test",
                true,
            ),
            (
                "https://api.example.com",
                "https://api.example.com:443/v1/test",
                true,
            ),
            (
                "https://api.example.com:443",
                "https://api.example.com/v1/test",
                true,
            ),
            (
                "https://api.example.com:443",
                "https://api.example.com:443/v1/test",
                true,
            ),
            // 端口不匹配测试
            (
                "https://api.example.com",
                "https://api.example.com:8443/v1/test",
                false,
            ),
            (
                "https://api.example.com:443",
                "https://api.example.com:8443/v1/test",
                false,
            ),
        ];

        for (base_url, request_url, should_match) in test_cases {
            let result = validate_request_url(request_url, base_url, false);

            if should_match {
                assert!(
                    result.is_ok(),
                    "应该匹配的URL被拒绝: base_url={}, request_url={}, error={}",
                    base_url,
                    request_url,
                    result.unwrap_err()
                );
            } else {
                assert!(
                    result.is_err(),
                    "应该不匹配的URL被允许: base_url={}, request_url={}",
                    base_url,
                    request_url
                );
            }
        }
    }

    #[test]
    fn infinite_loop_usage_script_is_interrupted_before_blocking_the_backend() {
        // 用量脚本来自不可信输入（deeplink / 同步导入的 DB 行），必须限制 CPU 时间，
        // 否则 `while(true)` 会挂死执行线程（DoS）。
        let script = r#"
            (function(){
                while (true) { Math.sqrt(Math.random()); }
            })();
            ({ request: { url: "https://example.com", method: "GET" } })
        "#;

        let start = std::time::Instant::now();
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("tokio runtime for test")
            .block_on(execute_usage_script(
                script,
                "sk-test",
                "https://api.example.com",
                30,
                None,
                None,
                None,
            ));
        let elapsed = start.elapsed();

        assert!(
            result.is_err(),
            "infinite loop script must be rejected, got: {result:?}"
        );
        // 必须明显短于无限等待；留足余量避免 CI 抖动，但应远小于 30 秒网络超时。
        assert!(
            elapsed < std::time::Duration::from_secs(15),
            "interruption took too long: {elapsed:?}"
        );
    }

    #[test]
    fn template_binding_keeps_special_secrets_out_of_javascript_source() {
        let script = r#"({
            request: {
                url: "{{baseUrl}}/usage",
                method: "GET",
                headers: { Authorization: "Bearer {{apiKey}}", "X-Token": '{{accessToken}}' },
                body: '{"user":"{{userId}}"}'
            }
        })"#;
        let bound = bind_template_vars(script);

        for secret in [
            "key-with-\"quote\\slash",
            "https://api.example.test",
            "token-with-newline\nvalue",
            "user-123",
        ] {
            assert!(!bound.contains(secret));
        }
        assert!(bound.contains("__ccsApiKey"));
        assert!(bound.contains("__ccsBaseUrl"));
        assert!(bound.contains("__ccsAccessToken"));
        assert!(bound.contains("__ccsUserId"));

        let runtime = Runtime::new().expect("runtime");
        let context = Context::full(&runtime).expect("context");
        context.with(|ctx| {
            let globals = ctx.globals();
            globals
                .set("__ccsApiKey", "key-with-\"quote\\slash")
                .unwrap();
            globals
                .set("__ccsBaseUrl", "https://api.example.test")
                .unwrap();
            globals
                .set("__ccsAccessToken", "token-with-newline\nvalue")
                .unwrap();
            globals.set("__ccsUserId", "user-123").unwrap();
            let _: rquickjs::Object = ctx.eval(bound.as_str()).expect("bound script parses");
        });
    }

    #[tokio::test]
    async fn http_error_preview_redacts_known_secret() {
        let secret = "sk-usage-script-secret-123456".to_string();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local test server");
        let address = listener.local_addr().expect("read local test address");
        let response_body = format!("upstream rejected {secret}");

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept test request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write test response");
        });

        let config = RequestConfig {
            url: format!("http://{address}/usage?apiKey={secret}"),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
        };
        let error = send_http_request(&config, 2, std::slice::from_ref(&secret))
            .await
            .expect_err("non-success response must be returned as an error")
            .to_string();

        server.await.expect("test server task");
        assert!(
            !error.contains(&secret),
            "HTTP errors must redact secrets: {error}"
        );
        assert!(
            error.contains("[REDACTED]"),
            "redaction marker missing: {error}"
        );
    }
}

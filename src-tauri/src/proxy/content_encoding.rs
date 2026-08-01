//! HTTP content-encoding 工具。
//!
//! reqwest 的自动解压已禁用（为了透传 accept-encoding），需要手动解压。
//! 请求侧（如 Codex Desktop 在登录态发压缩请求体）与响应侧（上游压缩响应体）
//! 共用同一套解压逻辑。

use axum::http::header::HeaderMap;
use std::io::Read;

/// 非流式响应在代理进程内允许的最大解压后体积。
///
/// 调用方若只需要更小的诊断体，应使用 [`decompress_body_with_limit`] 传入更低上限。
pub(crate) const MAX_DECOMPRESSED_BODY_BYTES: usize = 64 * 1024 * 1024;

/// 把 content-encoding 值拆成有序 coding 列表（去掉 identity 与空值）。
///
/// HTTP 允许堆叠编码（如 `gzip, zstd`），各 coding 以逗号分隔；亦允许重复
/// content-encoding 头，语义等同逗号拼接（见 [`get_content_encoding`]）。
fn split_codings(content_encoding: &str) -> Vec<&str> {
    content_encoding
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty() && *c != "identity")
        .collect()
}

/// 单个 coding 是否可被解压。
fn is_single_supported(coding: &str) -> bool {
    matches!(
        coding,
        "gzip" | "x-gzip" | "deflate" | "br" | "zstd" | "zst"
    )
}

/// 解压单个 content-coding。未知编码返回 `Ok(None)`。
#[derive(Debug)]
enum LimitedReadError {
    Read(std::io::Error),
    LimitExceeded { max_output_bytes: usize },
}

impl From<LimitedReadError> for std::io::Error {
    fn from(error: LimitedReadError) -> Self {
        match error {
            LimitedReadError::Read(error) => error,
            LimitedReadError::LimitExceeded { max_output_bytes } => std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("decompressed body exceeds the {max_output_bytes}-byte safety limit"),
            ),
        }
    }
}

fn read_to_end_limited(
    reader: impl Read,
    max_output_bytes: usize,
) -> Result<Vec<u8>, LimitedReadError> {
    let read_limit = u64::try_from(max_output_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut limited = reader.take(read_limit);
    let mut decompressed = Vec::new();
    limited
        .read_to_end(&mut decompressed)
        .map_err(LimitedReadError::Read)?;
    if decompressed.len() > max_output_bytes {
        return Err(LimitedReadError::LimitExceeded { max_output_bytes });
    }
    Ok(decompressed)
}

fn decompress_single(
    coding: &str,
    body: &[u8],
    max_output_bytes: usize,
) -> Result<Option<Vec<u8>>, std::io::Error> {
    match coding {
        "gzip" | "x-gzip" => {
            let decoder = flate2::read::GzDecoder::new(body);
            Ok(Some(
                read_to_end_limited(decoder, max_output_bytes).map_err(std::io::Error::from)?,
            ))
        }
        "deflate" => {
            // RFC 9110: deflate 指 zlib 包裹格式；但部分上游 / 客户端发 raw deflate 流。
            // 先按规范尝试 zlib，失败再回退 raw —— 否则合规来源必然解压失败，
            // 原始压缩字节会被 fail-open 透传给 JSON 解析（#2234 形态 C 之一）。
            let zlib = flate2::read::ZlibDecoder::new(body);
            match read_to_end_limited(zlib, max_output_bytes) {
                Ok(decompressed) => Ok(Some(decompressed)),
                Err(LimitedReadError::LimitExceeded { max_output_bytes }) => {
                    Err(LimitedReadError::LimitExceeded { max_output_bytes }.into())
                }
                Err(LimitedReadError::Read(zlib_err)) => {
                    log::debug!("deflate 按 zlib 解压失败（{zlib_err}），回退 raw deflate");
                    let raw = flate2::read::DeflateDecoder::new(body);
                    Ok(Some(
                        read_to_end_limited(raw, max_output_bytes).map_err(std::io::Error::from)?,
                    ))
                }
            }
        }
        "br" => {
            let decoder = brotli::Decompressor::new(body, 4096);
            Ok(Some(
                read_to_end_limited(decoder, max_output_bytes).map_err(std::io::Error::from)?,
            ))
        }
        "zstd" | "zst" => {
            // Codex 登录态对请求体启用 zstd（Compression::Zstd）；上游也可能 zstd 压缩响应。
            let decoder = zstd::stream::read::Decoder::new(std::io::Cursor::new(body))?;
            Ok(Some(
                read_to_end_limited(decoder, max_output_bytes).map_err(std::io::Error::from)?,
            ))
        }
        _ => Ok(None),
    }
}

/// 根据 content-encoding 解压 body 字节，支持堆叠编码（如 `gzip, zstd`）。
///
/// RFC 9110 §8.4：codings 按**应用顺序**列出，故解压须**反向**（最后应用的先解）。
/// 返回 `Ok(None)` 表示存在不受支持的编码、原样透传——此时调用方必须保留
/// content-encoding 头，否则下游（诊断 / 客户端）会把压缩字节误当明文。
pub(crate) fn decompress_body(
    content_encoding: &str,
    body: &[u8],
) -> Result<Option<Vec<u8>>, std::io::Error> {
    decompress_body_with_limit(content_encoding, body, MAX_DECOMPRESSED_BODY_BYTES)
}

/// 与 [`decompress_body`] 相同，但由调用方指定每一层解码允许产生的最大字节数。
///
/// 堆叠编码会在每个中间层应用同一上限，避免中间表示先无限膨胀、最终层再缩小的
/// 输入绕过内存保护。
pub(crate) fn decompress_body_with_limit(
    content_encoding: &str,
    body: &[u8],
    max_output_bytes: usize,
) -> Result<Option<Vec<u8>>, std::io::Error> {
    let codings = split_codings(content_encoding);
    if codings.is_empty() {
        return Ok(None);
    }
    // 任一 coding 不支持就整体放弃解压、保头透传，避免半解码的脏数据。
    if !codings.iter().all(|c| is_single_supported(c)) {
        log::warn!("不支持的 content-encoding: {content_encoding}，跳过解压");
        return Ok(None);
    }

    // 反向解码：列表末尾是最后应用的编码，须最先解。
    let mut data: Option<Vec<u8>> = None;
    for coding in codings.iter().rev() {
        let input = data.as_deref().unwrap_or(body);
        match decompress_single(coding, input, max_output_bytes)? {
            Some(decompressed) => data = Some(decompressed),
            // 上面 is_single_supported 已校验，理论不会发生；防御性兜底。
            None => return Ok(None),
        }
    }
    Ok(data)
}

/// 该 content-encoding（含堆叠，如 `gzip, zstd`）是否全部可被解压。
///
/// 请求侧用它做闸门：无法解压的压缩体不能透传给 JSON 解析，需直接拒绝。
pub(crate) fn is_supported_content_encoding(content_encoding: &str) -> bool {
    let codings = split_codings(content_encoding);
    !codings.is_empty() && codings.iter().all(|c| is_single_supported(c))
}

/// 从 header 提取 content-encoding（合并重复头，忽略 identity 与空值）。
///
/// HTTP 允许重复 content-encoding 头，语义等同逗号拼接，故用 `get_all` 合并；
/// 返回值可能含多个逗号分隔的 coding，交由 [`decompress_body`] 反向解码。
pub(crate) fn get_content_encoding(headers: &HeaderMap) -> Option<String> {
    let combined = headers
        .get_all("content-encoding")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
        .to_lowercase();
    if split_codings(&combined).is_empty() {
        return None;
    }
    Some(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn decompress_body_deflate_handles_zlib_wrapped_per_rfc9110() {
        // RFC 9110 规范的 deflate = zlib 包裹格式（合规来源发的就是这个）
        let payload = br#"{"ok":true}"#;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let decompressed = decompress_body("deflate", &compressed).unwrap().unwrap();
        assert_eq!(decompressed, payload);
    }

    #[test]
    fn decompress_body_deflate_falls_back_to_raw_stream() {
        // 部分来源违规发 raw deflate 流，保持兼容
        let payload = br#"{"ok":true}"#;
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let decompressed = decompress_body("deflate", &compressed).unwrap().unwrap();
        assert_eq!(decompressed, payload);
    }

    #[test]
    fn decompress_body_zstd_roundtrip() {
        // Codex 登录态发的就是 zstd 压缩请求体
        let payload = br#"{"hello":"world","n":42}"#;
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(&payload[..]), 0).unwrap();
        let decompressed = decompress_body("zstd", &compressed).unwrap().unwrap();
        assert_eq!(decompressed, payload);
    }

    #[test]
    fn decompress_body_stacked_gzip_then_zstd_decodes_in_reverse() {
        // Content-Encoding: gzip, zstd 表示先 gzip 后 zstd，解压须反向（先 zstd 后 gzip）
        let payload = br#"{"stacked":true}"#;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, payload).unwrap();
        let gzipped = gz.finish().unwrap();
        let stacked = zstd::stream::encode_all(std::io::Cursor::new(&gzipped[..]), 0).unwrap();

        let decompressed = decompress_body("gzip, zstd", &stacked).unwrap().unwrap();
        assert_eq!(decompressed, payload);
    }

    #[test]
    fn decompress_body_stacked_with_unsupported_returns_none() {
        // 堆叠里只要有一个不支持，就整体保头透传
        let result = decompress_body("snappy, zstd", b"\x00\x01\x02\x03").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn decompress_body_unknown_encoding_returns_none_to_keep_headers() {
        // 未知编码必须返回 None（而非伪装成"已解码"），否则 content-encoding
        // 头被剥掉，下游诊断会把压缩字节误报成明文
        let result = decompress_body("snappy", b"\x00\x01\x02\x03").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn decompress_body_rejects_output_above_limit() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &vec![b'x'; 1025]).unwrap();
        let compressed = encoder.finish().unwrap();

        let error = decompress_body_with_limit("gzip", &compressed, 1024)
            .expect_err("expanded body must be rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("1024-byte safety limit"));
    }

    #[test]
    fn deflate_limit_rejection_does_not_fall_back_to_raw_decoder() {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &vec![b'x'; 1025]).unwrap();
        let compressed = encoder.finish().unwrap();

        let error = decompress_body_with_limit("deflate", &compressed, 1024)
            .expect_err("expanded zlib body must be rejected without a raw-deflate fallback");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("1024-byte safety limit"));
    }

    #[test]
    fn is_supported_content_encoding_matches_decompressable() {
        for enc in [
            "gzip",
            "x-gzip",
            "deflate",
            "br",
            "zstd",
            "zst",
            "gzip, zstd",
        ] {
            assert!(is_supported_content_encoding(enc), "{enc} 应受支持");
        }
        for enc in ["identity", "snappy", "compress", "", "gzip, snappy"] {
            assert!(!is_supported_content_encoding(enc), "{enc} 不应受支持");
        }
    }

    #[test]
    fn get_content_encoding_combines_repeated_headers() {
        // 重复的 content-encoding 头等同逗号拼接，须用 get_all 合并
        let mut headers = HeaderMap::new();
        headers.append("content-encoding", HeaderValue::from_static("gzip"));
        headers.append("content-encoding", HeaderValue::from_static("zstd"));
        assert_eq!(
            get_content_encoding(&headers).as_deref(),
            Some("gzip, zstd")
        );
    }

    #[test]
    fn get_content_encoding_ignores_identity_only() {
        let mut headers = HeaderMap::new();
        headers.append("content-encoding", HeaderValue::from_static("identity"));
        assert_eq!(get_content_encoding(&headers), None);
    }
}

use std::io::Cursor;
use std::time::Duration;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

/// Per-request fetch timeout. The framework ceiling (`timeout_seconds`) is set
/// above this so a legitimate slow fetch is never pre-empted by the wrapper.
const FETCH_TIMEOUT_SECS: u64 = 30;
/// Framework wrapper ceiling (must be > FETCH_TIMEOUT_SECS).
const FRAMEWORK_TIMEOUT_CEILING_SECS: u64 = 60;
/// Maximum response body size (matches opencode's 5 MiB cap).
const MAX_BYTES: usize = 5 * 1024 * 1024;
/// Maximum characters of converted text returned to the model.
const MAX_OUTPUT_CHARS: usize = 30_000;
/// Browser-like User-Agent so trivial bot-detection doesn't block the fetch.
const BROWSER_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/143.0.0.0 Safari/537.36";

#[tool(
    name = "webfetch",
    description = "Fetches an HTTP(S) URL and returns its content as plain text. HTML pages are converted to text; the fetched content is returned for the model to analyze."
)]
pub struct WebFetchInput {
    #[desc = "The HTTP or HTTPS URL to fetch"]
    pub url: String,
}

pub struct WebFetchTool;

#[async_trait]
impl ToolFunction for WebFetchTool {
    type Input = WebFetchInput;

    fn timeout_seconds(&self) -> u64 {
        FRAMEWORK_TIMEOUT_CEILING_SECS
    }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // 1. Validate URL scheme.
        let parsed = match reqwest::Url::parse(&input.url) {
            Ok(u) if u.scheme() == "http" || u.scheme() == "https" => u,
            _ => {
                return Ok(ToolResult::error(
                    format!("URL must be http(s)://... : got {:?}", input.url),
                ));
            }
        };

        // 2. Build client with browser UA + fetch timeout. reqwest follows
        //    redirects by default (≤10 hops) and decompresses gzip/brotli.
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
            .user_agent(BROWSER_UA)
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::error(
                    format!("Failed to build HTTP client: {e}"),
                ));
            }
        };

        // 3. Send GET.
        let resp = match client.get(parsed).send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult::error(
                    format!("Failed to fetch: {e}"),
                ));
            }
        };

        // 4. Size guard (declared Content-Length first, then actual bytes).
        if let Some(len) = resp.content_length() {
            if len as usize > MAX_BYTES {
                return Ok(ToolResult::error(
                    format!("Response too large ({len} bytes, max {MAX_BYTES})"),
                ));
            }
        }
        // 5. Content-type dispatch (capture before bytes() consumes resp).
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();

        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult::error(
                    format!("Failed to read response body: {e}"),
                ));
            }
        };
        if bytes.len() > MAX_BYTES {
            return Ok(ToolResult::error(
                format!("Response too large ({} bytes, max {MAX_BYTES})", bytes.len()),
            ));
        }

        let text = match convert(&content_type, &bytes) {
            Ok(t) => t,
            Err(msg) => return Ok(ToolResult::error(msg)),
        };

        // 6. Truncate (head) and return.
        Ok(ToolResult::success(truncate_head(&text, MAX_OUTPUT_CHARS)))
    }
}

/// Convert a response body to plain text based on its content type.
///
/// - `text/html` → `html2text` (strips tags/scripts/styles).
/// - other textual types (text/*, json, xml, javascript, svg) → UTF-8 pass-through.
/// - anything else (images, pdf, octet-stream) → error.
fn convert(content_type: &str, bytes: &[u8]) -> Result<String, String> {
    let mime = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if mime == "text/html" || mime == "application/xhtml+xml" {
        let html = String::from_utf8_lossy(bytes);
        html2text::from_read(&mut Cursor::new(html.into_owned().into_bytes()), usize::MAX)
            .map_err(|e| e.to_string())
    } else if is_textual(&mime) {
        Ok(String::from_utf8_lossy(bytes).into_owned())
    } else {
        Err(format!("unsupported content type: {mime}"))
    }
}

/// True for MIME types safe to return as text.
fn is_textual(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime == "application/json"
        || mime == "application/xml"
        || mime == "application/javascript"
        || mime.ends_with("+json")
        || mime.ends_with("+xml")
        || mime == "image/svg+xml"
}

/// Cap `s` to `max` characters (head), appending a notice when truncated.
fn truncate_head(s: &str, max: usize) -> String {
    let total = s.chars().count();
    if total <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}\n\n... [output truncated, {total} chars total]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_html_strips_tags() {
        let html = b"<html><head><title>T</title><script>var x=1;</script></head>\
                     <body><h1>Hello</h1><p>World &amp; all</p></body></html>";
        let text = convert("text/html; charset=utf-8", html).unwrap();
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("<script>"));
        assert!(!text.contains("var x"));
    }

    #[test]
    fn test_convert_json_passthrough() {
        let body = br#"{"k": "v"}"#;
        let text = convert("application/json", body).unwrap();
        assert_eq!(text, "{\"k\": \"v\"}");
    }

    #[test]
    fn test_convert_binary_rejected() {
        let res = convert("application/pdf", b"%PDF-1.4");
        assert!(res.is_err());
    }

    #[test]
    fn test_truncate_head_short() {
        assert_eq!(truncate_head("abc", 10), "abc");
    }

    #[test]
    fn test_truncate_head_long() {
        let s = "a".repeat(100);
        let out = truncate_head(&s, 20);
        assert!(out.starts_with(&"a".repeat(20)));
        assert!(out.contains("100 chars total"));
    }

    #[tokio::test]
    async fn test_webfetch_rejects_invalid_scheme() {
        let tool = WebFetchTool;
        let result = tool
            .run(WebFetchInput {
                url: "ftp://example.com/file".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn test_webfetch_rejects_garbage() {
        let tool = WebFetchTool;
        let result = tool
            .run(WebFetchInput {
                url: "not a url".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn test_webfetch_live() {
        let tool = WebFetchTool;
        let result = tool
            .run(WebFetchInput {
                url: "https://example.com".to_string(),
            })
            .await
            .unwrap();
        assert!(result.is_error.is_none(), "fetch failed: {:?}", result.content);
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => {
                assert!(t.contains("Example Domain"), "got: {t}");
            }
            other => panic!("expected text, got {other:?}"),
        }
    }
}

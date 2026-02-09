use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

#[derive(Debug, Deserialize, Serialize)]
pub struct LspHeader {
    pub jsonrpc: String,
    pub method: Option<String>,
    pub id: Option<serde_json::Value>,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DidOpenParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentItem,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TextDocumentItem {
    pub uri: String,
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DidChangeParams {
    #[serde(rename = "textDocument")]
    pub text_document: VersionedTextDocumentIdentifier,
    #[serde(rename = "contentChanges")]
    pub content_changes: Vec<TextDocumentContentChangeEvent>,
}

#[derive(serde::Deserialize)]
pub struct DidCloseParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TextDocumentIdentifier {
    pub uri: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VersionedTextDocumentIdentifier {
    pub uri: String,
    pub version: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TextDocumentContentChangeEvent {
    pub range: Option<Range>,
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TextEdit {
    pub range: Range,
    #[serde(rename = "newText")]
    pub new_text: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CursorPositionParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InitializeParams {
    #[serde(rename = "rootUri")]
    pub root_uri: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InitializeResult {
    pub capabilities: ServerCapabilities,
}

#[derive(Debug, Serialize)]
pub struct ServerCapabilities {
    pub text_doc_sync: i32, // 1 = full, 2 = incremental
}

pub async fn read_message<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    let mut header_lines_read = 0;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            if header_lines_read > 0 {
                return Err(anyhow!("Connection closed mid-header!"));
            }
            return Ok(None);
        }

        header_lines_read += 1;

        if line.trim().is_empty() {
            break;
        }

        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("content-length") {
            content_length = Some(
                value
                    .trim()
                    .parse()
                    .context("Content-Length header is not a number")?,
            );
        }
    }

    let length = content_length.ok_or_else(|| anyhow!("Missing Content-Length header"))?;
    let mut body_buffer = vec![0; length];
    reader.read_exact(&mut body_buffer).await?;
    let body = String::from_utf8(body_buffer).context("LSP body was not valid UTF-8")?;

    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::io::BufReader;

    async fn run_parser(input: &[u8]) -> Result<Option<String>> {
        let cursor = Cursor::new(input);
        let mut reader = BufReader::new(cursor);
        read_message(&mut reader).await
    }

    // =========================================================================
    //  HAPPY PATHS (These remain the same)
    // =========================================================================

    #[tokio::test]
    async fn test_valid_simple_message() {
        let input = b"Content-Length: 5\r\n\r\nHello";
        let result = run_parser(input).await.unwrap();
        assert_eq!(result, Some("Hello".to_string()));
    }

    #[tokio::test]
    async fn test_valid_with_extra_headers() {
        let input =
            b"User-Agent: MockClient/1.0\r\ncontent-length: 5\r\nContent-Type: utf8\r\n\r\nWorld";
        let result = run_parser(input).await.unwrap();
        assert_eq!(result, Some("World".to_string()));
    }

    #[tokio::test]
    async fn test_valid_whitespace_tolerance() {
        let input = b"Content-Length:   4\r\n\r\ntest";
        let result = run_parser(input).await.unwrap();
        assert_eq!(result, Some("test".to_string()));
    }

    #[tokio::test]
    async fn test_valid_empty_body() {
        let input = b"Content-Length: 0\r\n\r\n";
        let result = run_parser(input).await.unwrap();
        assert_eq!(result, Some("".to_string()));
    }

    #[tokio::test]
    async fn test_clean_eof() {
        let input = b"";
        let result = run_parser(input).await.unwrap();
        assert_eq!(result, None);
    }

    // =========================================================================
    //  EDGE CASES & ERRORS
    // =========================================================================

    #[tokio::test]
    async fn test_error_missing_content_length() {
        let input = b"User-Agent: Test\r\n\r\nHello";
        let err = run_parser(input).await.unwrap_err();
        assert_eq!(err.to_string(), "Missing Content-Length header");
    }

    #[tokio::test]
    async fn test_error_malformed_header_value() {
        let input = b"Content-Length: five\r\n\r\nHello";
        let err = run_parser(input).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("Content-Length header is not a number")
        );
    }

    #[tokio::test]
    async fn test_error_body_too_short() {
        let input = b"Content-Length: 10\r\n\r\n12345";
        let err = run_parser(input).await.unwrap_err();
        assert!(
            err.downcast_ref::<std::io::Error>().unwrap().kind()
                == std::io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn test_error_invalid_utf8_body() {
        let input = b"Content-Length: 2\r\n\r\n\xFF\xFF";
        let err = run_parser(input).await.unwrap_err();
        assert_eq!(err.to_string(), "LSP body was not valid UTF-8");
    }

    // =========================================================================
    //  RESILIENCE TESTS
    // =========================================================================

    #[tokio::test]
    async fn test_truncated_headers_return_error() {
        // SCENARIO: Connection cuts halfway through headers.
        let input = b"Content-Length: 5\r\nUser-Age";
        let result = run_parser(input).await;

        // Assert that we now correctly catch this as an error
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Connection closed mid-header!"
        );
    }

    #[tokio::test]
    async fn test_colon_in_values_safe() {
        // SCENARIO: A header has multiple colons.
        let input = b"Host: localhost:8080\r\nContent-Length: 5\r\n\r\nHello";
        let result = run_parser(input).await.unwrap();

        assert_eq!(result, Some("Hello".to_string()));
    }
}

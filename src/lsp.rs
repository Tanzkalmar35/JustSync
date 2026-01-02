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

#[derive(serde::Deserialize)]
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

#[derive(Debug, Deserialize, Serialize, Clone)]
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

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            return Ok(None);
        }

        if line.trim().is_empty() {
            break;
        }

        if line.to_lowercase().starts_with("content-length:") {
            let content_length_dirty = line
                .split(":")
                .last()
                .ok_or_else(|| anyhow!("Content-Length header is malformed"))?;
            content_length = Some(content_length_dirty.trim().parse::<usize>()?);
        }
    }

    let length = content_length.ok_or_else(|| anyhow!("Missing Content-Length header"))?;
    let mut body_buffer = vec![0; length];
    reader.read_exact(&mut body_buffer).await?;
    let body = String::from_utf8(body_buffer).context("LSP body was not valid UTF-8")?;

    Ok(Some(body))
}

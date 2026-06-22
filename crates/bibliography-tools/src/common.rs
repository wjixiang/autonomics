//! Shared helpers for bibliography tools: subprocess bridge communication
//! and error mapping.

use agentik_core::tools::ToolError;
use anyhow::{Result, anyhow};
use serde_json::Value;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use std::process::Stdio;

/// Default path to the bibliography-search project directory.
/// Override with the BIBLIOGRAPHY_SEARCH_DIR env var at runtime.
const DEFAULT_BIBLIOGRAPHY_SEARCH_DIR: &str = "/mnt/disk1/project/bibliography-search";

/// Locate the `pubmed-bridge.mjs` script relative to the crate manifest dir.
fn bridge_script_path() -> Result<PathBuf> {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").map_err(|_| anyhow!("CARGO_MANIFEST_DIR not set"))?;
    Ok(PathBuf::from(manifest_dir).join("scripts").join("pubmed-bridge.mjs"))
}

/// Get the bibliography-search library directory.
fn lib_dir() -> String {
    std::env::var("BIBLIOGRAPHY_SEARCH_DIR").unwrap_or_else(|_| DEFAULT_BIBLIOGRAPHY_SEARCH_DIR.to_string())
}

/// Map any error into a tool execution error.
pub fn err<E>(source: E) -> ToolError
where
    E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
{
    ToolError::ExecutionFailed {
        source: source.into(),
    }
}

/// Bridge response envelope.
#[derive(Debug, serde::Deserialize)]
pub struct BridgeResponse {
    pub ok: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
}

/// Spawn the Node.js bridge script, send a JSON payload, and read the
/// JSON response.
pub async fn call_bridge(payload: &Value) -> Result<BridgeResponse> {
    let script = bridge_script_path()?;
    if !script.exists() {
        return Err(anyhow!(
            "bridge script not found: {}",
            script.display()
        ));
    }

    let lib = lib_dir();
    let lib_path = PathBuf::from(&lib).join("dist/index.js");
    if !lib_path.exists() {
        return Err(anyhow!(
            "bibliography-search dist/index.js not found at: {}",
            lib_path.display()
        ));
    }

    let mut child = Command::new("node")
        .arg(&script)
        .env("BIBLIOGRAPHY_SEARCH_DIR", &lib)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow!("failed to spawn bridge script: {e}"))?;

    // Write payload to stdin and close.
    let mut stdin_handle = child.stdin.take().expect("stdin was piped");
    let payload_str =
        serde_json::to_string(payload).map_err(|e| anyhow!("serialize payload: {e}"))?;
    stdin_handle.write_all(payload_str.as_bytes()).await?;
    stdin_handle.write_all(b"\n").await?;
    stdin_handle.shutdown().await?;

    // Read stdout.
    let mut stdout_buf = Vec::new();
    let mut stdout_handle = child.stdout.take().expect("stdout was piped");
    stdout_handle.read_to_end(&mut stdout_buf).await?;

    // Capture stderr for diagnostics.
    let mut stderr_buf = Vec::new();
    let mut stderr_handle = child.stderr.take().expect("stderr was piped");
    stderr_handle.read_to_end(&mut stderr_buf).await?;

    let status = child.wait().await?;
    if !status.success() {
        let stderr_text = String::from_utf8_lossy(&stderr_buf);
        return Err(anyhow!(
            "bridge script exited with {}: {}",
            status,
            stderr_text.trim()
        ));
    }

    let output = String::from_utf8_lossy(&stdout_buf);
    let response: BridgeResponse = serde_json::from_str(output.trim())
        .map_err(|e| anyhow!("parse bridge response: {e} (raw: {output})"))?;

    Ok(response)
}

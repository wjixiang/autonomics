use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::storage::OpendalFileStorage;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "file_list",
    description = "List entries under a path. Returns names, types (file/dir), and sizes."
)]
pub struct FileListInput {
    #[desc = "Directory path to list. Defaults to \"/\"."]
    pub path: Option<String>,
    #[desc = "List recursively. Defaults to true."]
    pub recursive: Option<bool>,
}

pub struct FileListTool {
    pub(crate) storage: Arc<OpendalFileStorage>,
}

#[async_trait]
impl ToolFunction for FileListTool {
    type Input = FileListInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let op = &self.storage.op;
        let path = input.path.unwrap_or_else(|| "/".to_string());
        let recursive = input.recursive.unwrap_or(true);

        let items = if recursive {
            let mut lister = op.lister(&path).await.map_err(|e| e.to_string())?;
            let mut items = Vec::new();
            while let Some(entry) = lister.next().await {
                let entry = entry.map_err(|e| e.to_string())?;
                let meta = entry.metadata();
                items.push(serde_json::json!({
                    "name": entry.path(),
                    "is_dir": meta.is_dir(),
                    "size": meta.content_length(),
                }));
            }
            items
        } else {
            let entries = op.list(&path).await.map_err(|e| e.to_string())?;
            entries
                .into_iter()
                .map(|entry| {
                    let meta = entry.metadata();
                    serde_json::json!({
                        "name": entry.path(),
                        "is_dir": meta.is_dir(),
                        "size": meta.content_length(),
                    })
                })
                .collect()
        };

        Ok(AgentToolResult::success_json(serde_json::json!({
            "path": path,
            "entries": items,
        })))
    }
}

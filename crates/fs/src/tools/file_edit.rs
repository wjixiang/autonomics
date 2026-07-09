use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;


use crate::storage::OpendalFileStorage;
use agentik_proc::tool;


#[tool(
    name = "file_edit",
    description = "Perform exact string replacement in a file. \
                  If replace_all is false, old_string must be unique in the file."
)]
pub struct FileEditInput {
    #[desc = "Path to the file to edit."]
    pub path: String,
    #[desc = "The text to replace."]
    pub old_string: String,
    #[desc = "The text to replace it with (must differ from old_string)."]
    pub new_string: String,
    #[desc = "Replace all occurrences instead of just the first. Defaults to false."]
    pub replace_all: Option<bool>,
}

pub struct FileEditTool {
    pub(crate) storage: Arc<OpendalFileStorage>,
}

#[async_trait]
impl ToolFunction for FileEditTool {
    type Input = FileEditInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        if input.old_string == input.new_string {
            return Err("old_string and new_string must be different".into());
        }

        let op = &self.storage.op;
        let path = OpendalFileStorage::normalize_path(&input.path);
        let buf = op.read(&path).await.map_err(|e| e.to_string())?;
        let text = String::from_utf8(buf.to_vec()).map_err(|e| e.to_string())?;

        let replace_all = input.replace_all.unwrap_or(false);
        let (new_text, replacements) = if replace_all {
            let count = text.matches(&input.old_string).count() as u64;
            (text.replace(&input.old_string, &input.new_string), count)
        } else {
            match text.find(&input.old_string) {
                None => return Err("old_string not found in file".into()),
                Some(idx) => {
                    if text[idx + input.old_string.len()..].contains(&input.old_string) {
                        return Err(
                            "old_string is not unique in file; \
                             use replace_all or provide more context"
                                .into(),
                        );
                    }
                    let mut new_text = String::with_capacity(text.len());
                    new_text.push_str(&text[..idx]);
                    new_text.push_str(&input.new_string);
                    new_text.push_str(&text[idx + input.old_string.len()..]);
                    (new_text, 1)
                }
            }
        };

        op.write(&path, new_text.into_bytes())
            .await
            .map_err(|e| e.to_string())?;

        Ok(AgentToolResult::success_json(serde_json::json!({
            "path": input.path,
            "replacements": replacements,
        })))
    }
}

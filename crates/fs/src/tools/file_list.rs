use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use futures::StreamExt;

use crate::storage::OpendalFileStorage;

#[derive(Debug)]
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
        let path = OpendalFileStorage::normalize_path(input.path.as_deref().unwrap_or("/"));
        let recursive = input.recursive.unwrap_or(true);

        let items = if recursive {
            let mut lister = op
                .lister_with(&path)
                .recursive(true)
                .await
                .map_err(|e| e.to_string())?;
            let mut items = Vec::new();
            while let Some(entry) = lister.next().await {
                let entry = entry.map_err(|e| e.to_string())?;
                let entry_path = entry.path().to_string();
                let meta = entry.metadata();
                let is_dir = meta.is_dir();
                // list() may not return accurate content_length; stat each entry
                // to get the real file size.
                let size = if is_dir {
                    0
                } else {
                    op.stat(&entry_path)
                        .await
                        .ok()
                        .map(|m| m.content_length())
                        .unwrap_or(meta.content_length())
                };
                items.push(serde_json::json!({
                    "name": entry_path,
                    "is_dir": is_dir,
                    "size": size,
                }));
            }
            items
        } else {
            // opendal Fs requires a trailing '/' to list children of a directory.
            let list_path = if path.ends_with('/') {
                path.clone()
            } else {
                format!("{path}/")
            };
            let mut lister = op
                .lister_with(&list_path)
                .recursive(false)
                .await
                .map_err(|e| e.to_string())?;
            let mut items = Vec::new();
            while let Some(entry) = lister.next().await {
                let entry = entry.map_err(|e| e.to_string())?;
                let meta = entry.metadata();
                let is_dir = meta.is_dir();
                let entry_path = entry.path().to_string();
                let size = if is_dir {
                    0
                } else {
                    op.stat(&entry_path)
                        .await
                        .ok()
                        .map(|m| m.content_length())
                        .unwrap_or(meta.content_length())
                };
                items.push(serde_json::json!({
                    "name": entry_path,
                    "is_dir": is_dir,
                    "size": size,
                }));
            }
            items
        };

        Ok(AgentToolResult::success_json(serde_json::json!({
            "path": path,
            "entries": items,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentik_sdk::types::ToolResultContent;

    /// Create a `FileListTool` backed by a temp directory pre-populated
    /// with the given `(path, content)` pairs.
    ///
    /// **Note:** opendal 0.57 Fs backend returns paths like `hello.txt`,
    /// `sub/world.txt`, `sub/` (no leading `/`).  The root is returned as `/`.
    async fn setup_tool(layout: Vec<(&str, &str)>) -> FileListTool {
        let storage = OpendalFileStorage::new_temp();
        for (path, content) in &layout {
            let bytes: Vec<u8> = content.as_bytes().to_vec();
            storage
                .op
                .write(path, opendal::Buffer::from(bytes))
                .await
                .unwrap();
        }
        FileListTool {
            storage: Arc::new(storage),
        }
    }

    /// Extract the JSON value from a successful `ToolResult`.
    fn result_json(result: AgentToolResult) -> serde_json::Value {
        match result.content {
            ToolResultContent::Json(v) => v,
            other => panic!("expected JSON content, got: {other:?}"),
        }
    }

    /// Collect the `name` field from every entry as a sorted `Vec<String>`.
    fn entry_names(entries: &serde_json::Value) -> Vec<String> {
        let mut names: Vec<String> = entries
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["name"].as_str().map(|s| s.to_string()))
            .collect();
        names.sort();
        names
    }

    /// Helper: assert that `entries` contains an entry whose `name` contains
    /// the given substring.
    fn assert_has_entry(entries: &serde_json::Value, substr: &str) {
        let names = entry_names(entries);
        assert!(
            names.iter().any(|n| n.contains(substr)),
            "expected entry containing '{substr}' in {names:?}"
        );
    }

    /// Helper: assert that `entries` does NOT contain an entry whose `name`
    /// contains the given substring.
    fn assert_no_entry(entries: &serde_json::Value, substr: &str) {
        let names = entry_names(entries);
        assert!(
            !names.iter().any(|n| n.contains(substr)),
            "did not expect entry containing '{substr}' in {names:?}"
        );
    }

    // ---------------------------------------------------------------
    // Recursive listing (default behaviour)
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn list_recursive_default() {
        let tool = setup_tool(vec![
            ("hello.txt", "hello"),
            ("sub/world.txt", "world"),
            ("sub/deep/nested.txt", "nested"),
        ])
        .await;

        let result = tool
            .run(FileListInput {
                path: None,
                recursive: None, // defaults to true
            })
            .await
            .unwrap();
        let json = result_json(result);

        // All three files must appear.
        assert_has_entry(&json["entries"], "hello.txt");
        assert_has_entry(&json["entries"], "sub/world.txt");
        assert_has_entry(&json["entries"], "sub/deep/nested.txt");
    }

    #[tokio::test]
    async fn list_recursive_explicit_true() {
        let tool = setup_tool(vec![("a.txt", "a"), ("dir/b.txt", "b")]).await;

        let result = tool
            .run(FileListInput {
                path: None,
                recursive: Some(true),
            })
            .await
            .unwrap();
        let json = result_json(result);

        assert_has_entry(&json["entries"], "a.txt");
        assert_has_entry(&json["entries"], "dir/b.txt");
    }

    #[tokio::test]
    async fn list_recursive_specific_subdir() {
        let tool = setup_tool(vec![
            ("root/file1.txt", "f1"),
            ("root/sub/file2.txt", "f2"),
            ("other.txt", "other"),
        ])
        .await;

        let result = tool
            .run(FileListInput {
                path: Some("/root".into()),
                recursive: Some(true),
            })
            .await
            .unwrap();
        let json = result_json(result);
        assert_eq!(json["path"], "/root");

        assert_has_entry(&json["entries"], "file1.txt");
        assert_has_entry(&json["entries"], "file2.txt");
        // other.txt must not appear — it's outside /root.
        assert_no_entry(&json["entries"], "other.txt");
    }

    // ---------------------------------------------------------------
    // Non-recursive listing
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn list_non_recursive() {
        let tool = setup_tool(vec![
            ("top/file1.txt", "f1"),
            ("top/file2.txt", "f2"),
            ("top/sub/deep.txt", "deep"),
        ])
        .await;

        let result = tool
            .run(FileListInput {
                path: Some("/top".into()),
                recursive: Some(false),
            })
            .await
            .unwrap();
        let json = result_json(result);

        // Direct children appear.
        assert_has_entry(&json["entries"], "file1.txt");
        assert_has_entry(&json["entries"], "file2.txt");
        // The deeply nested file must NOT appear.
        assert_no_entry(&json["entries"], "deep.txt");
    }

    // ---------------------------------------------------------------
    // Empty / fresh directory
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn list_fresh_temp_dir() {
        let storage = OpendalFileStorage::new_temp();
        let tool = FileListTool {
            storage: Arc::new(storage),
        };

        let result = tool
            .run(FileListInput {
                path: None,
                recursive: None,
            })
            .await
            .unwrap();
        let json = result_json(result);
        let entries = json["entries"].as_array().unwrap();

        // opendal Fs returns only the root "/" entry for an empty dir.
        assert!(
            entries.len() <= 1,
            "expected at most root entry, got: {entries:?}"
        );
        if !entries.is_empty() {
            assert_eq!(entries[0]["name"], "/");
            assert_eq!(entries[0]["is_dir"], true);
        }
    }

    // ---------------------------------------------------------------
    // File size accuracy
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn list_reports_file_size() {
        let tool = setup_tool(vec![("sized.txt", "hello world")]).await;

        let result = tool
            .run(FileListInput {
                path: None,
                recursive: Some(true),
            })
            .await
            .unwrap();
        let json = result_json(result);

        let file_entry = json["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| {
                e["name"]
                    .as_str()
                    .is_some_and(|n| n.contains("sized.txt"))
            })
            .expect("expected sized.txt entry");
        assert_eq!(file_entry["size"].as_u64().unwrap(), 11); // "hello world"
    }

    // ---------------------------------------------------------------
    // is_dir field
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn list_reports_is_dir() {
        let tool = setup_tool(vec![("with_dir/file.txt", "f"), ("other.txt", "o")]).await;

        let result = tool
            .run(FileListInput {
                path: None,
                recursive: Some(true),
            })
            .await
            .unwrap();
        let json = result_json(result);
        let entries = json["entries"].as_array().unwrap();

        // File entry — is_dir = false
        let file = entries
            .iter()
            .find(|e| e["name"].as_str().is_some_and(|n| n.contains("file.txt")))
            .expect("expected file.txt entry");
        assert!(!file["is_dir"].as_bool().unwrap());

        // At least one directory entry (e.g. "with_dir/") — is_dir = true, size = 0.
        let dir = entries
            .iter()
            .find(|e| e["is_dir"].as_bool() == Some(true))
            .expect("expected at least one directory entry");
        assert_eq!(dir["size"].as_u64().unwrap(), 0);
    }

    // ---------------------------------------------------------------
    // Path normalisation
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn list_path_normalisation() {
        let tool = setup_tool(vec![("foo.txt", "foo")]).await;

        // Relative path → normalised with leading "/".
        let result = tool
            .run(FileListInput {
                path: Some("foo.txt".into()),
                recursive: Some(true),
            })
            .await
            .unwrap();
        assert_eq!(result_json(result)["path"], "/foo.txt");

        // "./" relative path → leading "./" stripped, then normalised.
        let result = tool
            .run(FileListInput {
                path: Some("./foo.txt".into()),
                recursive: Some(true),
            })
            .await
            .unwrap();
        assert_eq!(result_json(result)["path"], "/foo.txt");

        // Bare "/" stays as "/".
        let result = tool
            .run(FileListInput {
                path: Some("/".into()),
                recursive: Some(true),
            })
            .await
            .unwrap();
        assert_eq!(result_json(result)["path"], "/");
    }

    // ---------------------------------------------------------------
    // Non-existent path — opendal returns empty (no error)
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn list_nonexistent_path_returns_empty() {
        let storage = OpendalFileStorage::new_temp();
        let tool = FileListTool {
            storage: Arc::new(storage),
        };

        let result = tool
            .run(FileListInput {
                path: Some("/does_not_exist".into()),
                recursive: Some(false),
            })
            .await
            .unwrap();
        let json = result_json(result);
        let entries = json["entries"].as_array().unwrap();
        assert!(
            entries.is_empty(),
            "expected empty entries for non-existent path, got: {entries:?}"
        );
    }
}

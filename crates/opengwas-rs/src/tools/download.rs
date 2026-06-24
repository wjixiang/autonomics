use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::json_err;
use crate::{OpengwasClient, types::GwasInfoFilesRequest};
use file_base::OpendalFileStorage;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "opengwas_download_files",
    description = "Download dataset files (summary stats .vcf.gz, index .vcf.gz.tbi, \
                  QC report _report.html) to local storage. Provide GWAS dataset \
                  IDs and the files will be streamed to the configured storage."
)]
pub struct DownloadFilesInput {
    #[desc = "List of GWAS study IDs to download files for, e.g. ['ieu-a-2', 'ukb-b-19953']."]
    pub id: Vec<String>,
}

pub struct DownloadFilesTool {
    pub(crate) client: Arc<OpengwasClient>,
    pub(crate) storage: Arc<OpendalFileStorage>,
}

#[async_trait]
impl ToolFunction for DownloadFilesTool {
    type Input = DownloadFilesInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        // 1. Get download URLs from the API.
        let resp = self
            .client
            .gwasinfo_files(&GwasInfoFilesRequest {
                id: input.id.clone(),
                commercial_approval_received: None,
            })
            .await
            .map_err(json_err)?;

        // 2. Download each file into storage.
        let mut downloaded = Vec::new();

        // The API response is typically a map: { "ieu-a-2": { "vcf.gz": "...", ... }, ... }
        if let Some(map) = resp.as_object() {
            for (study_id, files) in map {
                if let Some(file_urls) = files.as_object() {
                    for (filename, url_val) in file_urls {
                        if let Some(url) = url_val.as_str() {
                            let storage_path = format!("/{study_id}/{filename}");
                            let size = self
                                .client
                                .download_file_to_storage(url, &self.storage, &storage_path)
                                .await
                                .map_err(json_err)?;
                            downloaded.push(serde_json::json!({
                                "study_id": study_id,
                                "filename": filename,
                                "path": storage_path,
                                "size": size,
                            }));
                        }
                    }
                }
            }
        }

        Ok(AgentToolResult::success_json(serde_json::json!({
            "count": downloaded.len(),
            "files": downloaded,
        })))
    }
}

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::json_err;
use crate::format::format_download;
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

/// Flatten the nested JSON response into `(study_id, filename, url)` triples.
///
/// The API returns `{ study_id: ["url1", "url2", ...] }` where each URL ends
/// with the filename (e.g. `.../ieu-a-2.vcf.gz`).
fn parse_file_entries(resp: &serde_json::Value) -> Vec<(String, String, String)> {
    resp.as_object()
        .into_iter()
        .flatten()
        .flat_map(|(study_id, files)| {
            files.as_array().into_iter().flatten().filter_map(move |url_val| {
                let url = url_val.as_str()?;
                let filename = url.rsplit('/').next()?;
                Some((study_id.clone(), filename.to_string(), url.to_string()))
            })
        })
        .collect()
}

pub struct DownloadFilesTool {
    client: Arc<OpengwasClient>,
    storage: Arc<OpendalFileStorage>,
}

impl DownloadFilesTool {
    pub fn new(client: Arc<OpengwasClient>, storage: Arc<OpendalFileStorage>) -> Self {
        Self { client, storage }
    }
}

#[async_trait]
impl ToolFunction for DownloadFilesTool {
    type Input = DownloadFilesInput;

    fn sync_seconds(&self) -> u64 {
        1
    }

    fn timeout_seconds(&self) -> u64 {
        6000
    }

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

        for (study_id, filename, url) in parse_file_entries(&resp) {
            let storage_path = format!("/{study_id}/{filename}");
            let size = self
                .client
                .download_file_to_storage(&url, &self.storage, &storage_path)
                .await
                .map_err(json_err)?;
            downloaded.push(serde_json::json!({
                "study_id": study_id,
                "filename": filename,
                "path": storage_path,
                "size": size,
            }));
        }

        Ok(AgentToolResult::success(format_download(&serde_json::json!({
            "count": downloaded.len(),
            "files": downloaded,
        }))))
    }
}

//! Agent tool: ingest a CSV / TSV file into a named dataset.

use std::path::Path;
use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use anyhow::anyhow;
use async_trait::async_trait;
use data_engine::{Dataset, DatasetStore, Provenance};
use serde::{Deserialize, Serialize};

use crate::registry::IngestRegistry;
use crate::trait_def::IngestConfig;

/// Map any error into a [`ToolError::ExecutionFailed`].
fn err<E: std::error::Error + Send + Sync + 'static>(e: E) -> ToolError {
    ToolError::ExecutionFailed {
        source: Box::new(e),
    }
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "ingest_csv",
    description = "Parse a CSV or TSV file into an Arrow dataset. Schema is auto-inferred from the first rows. Supports .csv (comma-separated), .tsv (tab-separated), and .txt (tab-separated). The resulting dataset is registered in the store under the given name and can be referenced by subsequent dataset tools."
)]
pub struct IngestCsvInput {
    #[desc = "Path to the CSV or TSV file to ingest"]
    pub path: String,
    #[desc = "Name to register the resulting dataset under"]
    pub name: String,
    #[desc = "Batch size for Arrow RecordBatch construction. Defaults to 8192."]
    pub batch_size: Option<usize>,
    #[desc = "Maximum number of rows to ingest. None means all rows."]
    pub limit: Option<usize>,
}

pub struct IngestCsvTool {
    pub store: Arc<DatasetStore>,
}

#[async_trait]
impl ToolFunction for IngestCsvTool {
    type Input = IngestCsvInput;

    fn timeout_seconds(&self) -> u64 {
        600
    }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let path = input.path.trim();
        let name = input.name.trim();

        if path.is_empty() || name.is_empty() {
            return Ok(ToolResult::error("'path' and 'name' are both required"));
        }

        let file_path = Path::new(path);
        if !file_path.exists() {
            return Ok(ToolResult::error(format!("file not found: {path}")));
        }

        let config = IngestConfig {
            batch_size: input.batch_size.unwrap_or(8192),
            limit: input.limit,
        };

        let registry = IngestRegistry::with_defaults();
        let batches = registry
            .ingest_file(file_path, config)
            .await
            .map_err(|e| ToolError::from(anyhow!("{e}")))?;

        if batches.is_empty() {
            return Ok(ToolResult::error(format!("no records found in '{path}'")));
        }

        let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        let column_count = batches[0].schema().fields().len();

        let format_name = registry
            .find_ingestor(file_path)
            .map(|i| i.format_name().to_owned())
            .unwrap_or_else(|_| "CSV".into());

        let dataset = Dataset::new(batches)
            .map_err(err)?
            .with_provenance(Provenance::FileIngest {
                path: path.to_owned(),
                format: format_name,
            });

        self.store.put(dataset).await.map_err(err)?;

        // Re-read from store to get summary info.
        let ds = self.store.get(name).await.map_err(err)?;

        let schema = ds.schema_json();
        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": name,
            "source": path,
            "format": "CSV",
            "row_count": total_rows,
            "column_count": column_count,
            "schema": schema["columns"],
        })))
    }
}

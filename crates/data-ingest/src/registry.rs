//! Format registry — maps file extensions to the correct `FileIngestor`.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::csv::CsvIngestor;
use crate::error::IngestError;
use crate::trait_def::{FileIngestor, IngestConfig};
use crate::vcf::VcfIngestor;

/// Registry that maps file extensions to the correct [`FileIngestor`].
pub struct IngestRegistry {
    ingestors: Vec<Arc<dyn FileIngestor>>,
}

impl IngestRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            ingestors: Vec::new(),
        }
    }

    /// Create a registry pre-loaded with all built-in format ingestors.
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register(VcfIngestor);
        reg.register(CsvIngestor);
        reg
    }

    /// Register a format ingestor.
    pub fn register(&mut self, ingestor: impl FileIngestor + 'static) {
        self.ingestors.push(Arc::new(ingestor));
    }

    /// Find the right ingestor for a file path by its extension.
    ///
    /// Handles compound extensions like `.vcf.gz`.
    pub fn find_ingestor(&self, path: &Path) -> Result<Arc<dyn FileIngestor>, IngestError> {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        for ingestor in &self.ingestors {
            for supported in ingestor.file_extensions() {
                if file_name.ends_with(&format!(".{supported}")) || ext == *supported {
                    return Ok(Arc::clone(ingestor));
                }
            }
        }

        Err(IngestError::UnsupportedFormat {
            path: path.display().to_string(),
            extension: ext.to_owned(),
        })
    }

    /// Ingest a file using the registry to pick the right ingestor.
    pub async fn ingest_file(
        &self,
        path: &Path,
        config: IngestConfig,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        let ingestor = self.find_ingestor(path)?;
        ingestor.ingest(path, config).await
    }

    /// List all registered format names.
    pub fn supported_formats(&self) -> Vec<&str> {
        self.ingestors.iter().map(|i| i.format_name()).collect()
    }
}

impl Default for IngestRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

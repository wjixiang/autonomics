//! Error types for the data-ingest crate.

use std::path::Path;

use thiserror::Error;

/// Errors that can occur during file ingestion.
#[derive(Error, Debug)]
pub enum IngestError {
    /// No ingestor registered for this file format.
    #[error("unsupported file format: '{path}' (extension: '{extension}')")]
    UnsupportedFormat { path: String, extension: String },

    /// VCF-specific parsing error.
    #[error("VCF error: {0}")]
    Vcf(String),

    /// Arrow schema or batch construction error.
    #[error("Arrow error: {0}")]
    Arrow(String),

    /// I/O error reading the source file.
    #[error("I/O error reading '{path}': {message}")]
    Io { path: String, message: String },

    /// The source file is empty or has no records.
    #[error("no records found in '{path}'")]
    EmptyFile { path: String },

    /// Error building the output dataset.
    #[error("failed to build dataset: {0}")]
    Build(String),
}

impl IngestError {
    /// Wrap a VCF error with file-path context.
    pub fn vcf_with_path(path: &Path, err: impl Into<String>) -> Self {
        Self::Vcf(format!("{}: {}", path.display(), err.into()))
    }

    /// Wrap an Arrow error with file-path context.
    pub fn arrow_with_path(path: &Path, err: impl Into<String>) -> Self {
        Self::Arrow(format!("{}: {}", path.display(), err.into()))
    }
}

impl From<std::io::Error> for IngestError {
    fn from(e: std::io::Error) -> Self {
        Self::Io {
            path: String::new(),
            message: e.to_string(),
        }
    }
}

impl From<vcf_arrow::error::VcfError> for IngestError {
    fn from(e: vcf_arrow::error::VcfError) -> Self {
        Self::Vcf(e.to_string())
    }
}

impl From<arrow::error::ArrowError> for IngestError {
    fn from(e: arrow::error::ArrowError) -> Self {
        Self::Arrow(e.to_string())
    }
}

impl From<datalake::DatasetError> for IngestError {
    fn from(e: datalake::DatasetError) -> Self {
        Self::Build(e.to_string())
    }
}

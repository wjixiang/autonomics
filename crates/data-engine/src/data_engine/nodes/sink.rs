//! Unified sink node: consumes an upstream `DataFrame` and writes it out.
//!
//! Symmetric to [`crate::data_engine::nodes::SourceNode`]: a [`SinkNode`] has
//! exactly one input and produces no output. The destination is described by
//! [`Sink`] — a file (CSV / Parquet) or an Iceberg table.

use async_trait::async_trait;
use datafusion::common::HashMap;
use datafusion::common::config::{CsvOptions, TableParquetOptions};
use datafusion::dataframe::DataFrameWriteOptions;
use thiserror::Error;

use super::meta::{DagNode, NodeInput, NodeMeta};
use super::source::normalize_path;
use crate::data_engine::dag::DagError;
use crate::data_engine::dag::graph::NamedDataFrames;

/// Where a [`SinkNode`] writes to.
#[derive(Debug, Clone)]
pub enum Sink {
    /// Write to a file path or URL.
    File { path: String, format: WriteFormat },
    /// Write to an Iceberg table (catalog write path must be available).
    Iceberg { ident: String },
}

/// Supported on-disk write formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteFormat {
    Csv,
    Parquet,
}

#[derive(Debug, Error)]
pub enum SinkError {
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
    #[error("write sink '{path}' failed")]
    Write {
        path: String,
        #[source]
        source: datafusion::error::DataFusionError,
    },
}

impl From<SinkError> for DagError {
    fn from(e: SinkError) -> Self {
        match e {
            SinkError::Write { source, .. } => DagError::DataFusion(source),
            SinkError::InvalidInput { message } => DagError::Schedule(message),
        }
    }
}

pub struct SinkNode {
    meta: NodeMeta,
    sink: Sink,
}

impl SinkNode {
    pub fn new(meta: NodeMeta, sink: Sink) -> Self {
        Self { meta, sink }
    }
}

#[async_trait]
impl DagNode for SinkNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        let cp_node = Self {
            meta: self.meta.clone(),
            sink: self.sink.clone(),
        };

        Box::new(cp_node)
    }

    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<NamedDataFrames, DagError> {
        let input = inputs.first().ok_or(SinkError::InvalidInput {
            message: "SinkNode requires exactly one upstream input".to_string(),
        })?;

        match &self.sink {
            Sink::File { path, format } => {
                let path = normalize_path(path);
                let df = input.data.clone();
                let res = match format {
                    WriteFormat::Csv => {
                        df.write_csv(&path, DataFrameWriteOptions::default(), None::<CsvOptions>)
                            .await
                    }
                    WriteFormat::Parquet => {
                        df.write_parquet(
                            &path,
                            DataFrameWriteOptions::default(),
                            None::<TableParquetOptions>,
                        )
                        .await
                    }
                };
                res.map_err(|e| SinkError::Write {
                    path: path.clone(),
                    source: e,
                })?;
            }
            Sink::Iceberg { ident } => {
                // Iceberg table writes go through the catalog's writer. This is
                // a stub for now: a fully-qualified INSERT is not generally
                // available without a writable table provider, so surface a
                // clear error until the iceberg write path is wired in.
                return Err(SinkError::InvalidInput {
                    message: format!(
                        "Iceberg sink '{ident}' is not yet implemented; use File sink"
                    ),
                }
                .into());
            }
        }
        Ok(HashMap::new())
    }
}

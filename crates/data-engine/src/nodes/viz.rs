//! Visualization node: consumes an upstream `DataFrame` and renders it to a
//! PNG via R/ggplot2.
//!
//! Mirrors [`crate::nodes::SinkNode`]: a [`VizNode`] has exactly one input and
//! produces no output. It collects the upstream `RecordBatch`es, hands them to
//! the `visualization` crate (Arrow IPC → `Rscript` ggplot2), and writes a PNG
//! to `output_path`. The rendered path is surfaced to the agent via
//! `NodeReport.artifact_path` (see [`crate::dag::graph`]).

use std::path::PathBuf;

use async_trait::async_trait;
use datafusion::common::HashMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::meta::{DagNode, NodeInput, NodePorts};
use crate::dag::graph::PortOutputs;
use crate::dag::DagError;

/// Errors raised by the visualization node.
#[derive(Debug, Error)]
pub enum VizError {
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
    #[error("Visualization render failed")]
    Render {
        #[source]
        source: visualization::error::VizError,
    },
}

impl From<VizError> for DagError {
    fn from(e: VizError) -> Self {
        match e {
            VizError::InvalidInput { message } => DagError::Schedule(message),
            VizError::Render { source } => DagError::NodeError {
                node_type: "visualization".to_string(),
                msg: source.to_string(),
            },
        }
    }
}

/// Spec for a [`VizNode`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VizNodeSpec {
    /// Path the rendered PNG is written to. Its parent directory must exist
    /// and be writable.
    pub output_path: String,
    /// ggplot2 R code that runs with a `data.frame` named `df` already bound
    /// to the input data. Must build a ggplot object and assign it to a
    /// variable named `p`. Example:
    /// `p <- ggplot(df, aes(x = bp, y = pval)) + geom_point()`
    pub r_code: String,
    /// Figure width in inches (default 8).
    #[serde(default)]
    pub width: Option<f64>,
    /// Figure height in inches (default 6).
    #[serde(default)]
    pub height: Option<f64>,
    /// Resolution in DPI (default 150).
    #[serde(default)]
    pub dpi: Option<f64>,
}

pub struct VizNode {
    meta: NodePorts,
    output_path: String,
    r_code: String,
    width: Option<f64>,
    height: Option<f64>,
    dpi: Option<f64>,
}

/// Static port layout for every [`VizNode`]: a single untyped input port and
/// no outputs — identical to [`crate::nodes::SinkNode`].
fn port_layout() -> NodePorts {
    NodePorts::new().add_input_port(None)
}

impl VizNode {
    pub fn new(spec: VizNodeSpec) -> Self {
        Self {
            meta: port_layout(),
            output_path: spec.output_path,
            r_code: spec.r_code,
            width: spec.width,
            height: spec.height,
            dpi: spec.dpi,
        }
    }

    /// The path the PNG is rendered to (used to populate the node report).
    pub fn output_path(&self) -> &str {
        &self.output_path
    }
}

pub struct VizNodeFactory {}

impl crate::node_registry::registry::NodeFactory for VizNodeFactory {
    fn kind(&self) -> &'static str {
        "visualization"
    }

    fn desc(&self) -> &'static str {
        "Renders an upstream DataFrame to a PNG via R/ggplot2 (needs Rscript)."
    }

    fn doc(&self) -> &'static str {
        "A visualization node that consumes an upstream DataFrame, hands it to \
        R's ggplot2 (via the Arrow IPC stream format) and writes a PNG to \
        `output_path`. The `r_code` field runs with a data.frame named `df` \
        bound to the input data and must assign a ggplot object to `p`. \
        Requires `Rscript` on PATH with the `arrow` and `ggplot2` R packages \
        installed. One untyped input port; no output ports. The rendered path \
        is returned in the node report's `artifact_path` field."
    }

    fn spec_schema(&self) -> schemars::Schema {
        schemars::schema_for!(VizNodeSpec)
    }

    fn ports(&self) -> NodePorts {
        port_layout()
    }

    fn build(
        &self,
        spec: serde_json::Value,
        _node_ctx: crate::node_registry::registry::NodeCtx,
    ) -> crate::node_registry::error::Result<Box<dyn DagNode>> {
        let node_spec: VizNodeSpec = serde_json::from_value(spec)?;
        Ok(Box::new(VizNode::new(node_spec)))
    }
}

#[async_trait]
impl DagNode for VizNode {
    fn ports(&self) -> &NodePorts {
        &self.meta
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        let cp = Self {
            meta: self.meta.clone(),
            output_path: self.output_path.clone(),
            r_code: self.r_code.clone(),
            width: self.width,
            height: self.height,
            dpi: self.dpi,
        };
        Box::new(cp)
    }

    fn kind(&self) -> &'static str {
        "visualization"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<PortOutputs, DagError> {
        let input = inputs.first().ok_or(VizError::InvalidInput {
            message: "VizNode requires exactly one upstream input".to_string(),
        })?;

        // Collect the upstream DataFrame to concrete RecordBatches for the
        // renderer. This is the eager materialization point — visualization
        // needs all rows in memory to draw them.
        let batches = input
            .data
            .clone()
            .collect()
            .await
            .map_err(|e| DagError::NodeError {
                node_type: "visualization".to_string(),
                msg: format!("collecting input failed: {e}"),
            })?;

        let path = PathBuf::from(&self.output_path);
        visualization::render::render_png(
            &batches,
            &self.r_code,
            &path,
            self.width,
            self.height,
            self.dpi,
        )
        .await
        .map_err(|source| VizError::Render { source })?;

        // No DataFrame output — like SinkNode.
        Ok(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{Float64Array, Int32Array, RecordBatch};
    use arrow_schema::{DataType, Field, Schema};
    use datafusion::prelude::SessionContext;
    use std::sync::Arc;

    /// Build a small in-memory DataFrame for viz tests.
    fn sample_dataframe() -> datafusion::dataframe::DataFrame {
        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![
            Field::new("x", DataType::Int32, false),
            Field::new("y", DataType::Float64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![1, 2, 3, 4, 5])),
                Arc::new(Float64Array::from(vec![1.0, 4.0, 9.0, 16.0, 25.0])),
            ],
        )
        .expect("sample batch");
        ctx.read_batch(batch).expect("ctx reads batch")
    }

    /// The node renders its input to a real PNG and reports the output path.
    /// Requires `Rscript` (with `arrow`/`ggplot2`) on PATH.
    #[tokio::test]
    async fn test_viz_node_renders_png() {
        let out = format!("/tmp/viz_node_{}.png", std::process::id());
        let mut node = VizNode::new(VizNodeSpec {
            output_path: out.clone(),
            r_code: "p <- ggplot(df, aes(x = x, y = y)) + geom_point() + geom_line()".to_string(),
            width: Some(6.0),
            height: Some(4.0),
            dpi: Some(100.0),
        });

        let df = sample_dataframe();
        let res = node
            .execute(&[NodeInput { port: 0, data: df }])
            .await
            .expect("execute should succeed");
        assert!(res.is_empty(), "viz node has no output ports");

        let bytes = std::fs::read(&out).expect("read rendered png");
        assert!(bytes.len() > 100);
        assert_eq!(&bytes[0..4], &[0x89, b'P', b'N', b'G']);
        assert_eq!(node.output_path(), out);
        eprintln!("VizNode OK: {} bytes at {out}", bytes.len());
        let _ = std::fs::remove_file(&out);
    }
}

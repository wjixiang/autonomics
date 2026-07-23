use std::path::PathBuf;
use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use crate::ExecError;

/// Render a node's output DataFrame to a PNG via R/ggplot2, immediately.
///
/// Unlike the `visualization` DAG node (which runs as part of `run_dag`),
/// this tool plots the output of an *already-executed* node on demand — no
/// need to add a viz node + edge and re-run the DAG. Useful for ad-hoc
/// inspection of intermediate results.
///
/// Requires `Rscript` on PATH with the `arrow` and `ggplot2` R packages
/// installed.
#[tool(
    name = "viz",
    description = "Render an executed node's output DataFrame to a PNG via \
                  R/ggplot2, on demand (no DAG edit needed). The node must \
                  have been run (status Success). `r_code` runs with a \
                  data.frame named `df` bound to the node's primary output and \
                  must assign a ggplot object to `p`. Requires Rscript + the \
                  arrow/ggplot2 R packages. Returns the rendered `artifact_path`."
)]
pub struct VizInput {
    /// The node id whose output to plot. Must have been executed (Success).
    pub id: String,
    /// ggplot2 R code. A `data.frame` named `df` is bound to the node's
    /// primary output. Must build a ggplot object and assign it to `p`.
    /// Example: `p <- ggplot(df, aes(x = bp, y = pval)) + geom_point()`
    pub r_code: String,
    /// Path the rendered PNG is written to. Parent directory must exist.
    pub output_path: String,
    /// Figure width in inches (default 8).
    pub width: Option<f64>,
    /// Figure height in inches (default 6).
    pub height: Option<f64>,
    /// Resolution in DPI (default 150).
    pub dpi: Option<f64>,
}

pub struct VizTool {
    client: Arc<DataEngineClient>,
}

impl VizTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for VizTool {
    type Input = VizInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // Fetch the node's output DataFrames.
        let dfs = self
            .client
            .get_output(input.id.clone())
            .await
            .map_err(ExecError::from)?;

        let Some(dfs) = dfs else {
            return Ok(ToolResult::error(format!(
                "no output found for node '{}' — run the DAG first",
                input.id
            )));
        };

        // Plot the primary (first) output port. A node with zero output ports
        // (e.g. a sink) has nothing to plot.
        let (port_name, df) = dfs
            .iter()
            .next()
            .ok_or_else(|| ToolError::ExecutionFailed {
                source: Box::new(ExecError::Format(format!(
                    "node '{}' has no output ports to plot",
                    input.id
                ))),
            })?;

        // Materialize the DataFrame to RecordBatches for the renderer. This is
        // the eager point — visualization needs all rows in memory to draw.
        let batches = df
            .clone()
            .collect()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                source: Box::new(ExecError::Format(format!(
                    "collecting output of node '{}' failed: {e}",
                    input.id
                ))),
            })?;

        let path = PathBuf::from(&input.output_path);
        visualization::render::render_png(
            &batches,
            &input.r_code,
            &path,
            input.width,
            input.height,
            input.dpi,
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            source: Box::new(ExecError::Format(format!("{e}"))),
        })?;

        let content = serde_json::json!({
            "node": input.id,
            "output_port": port_name,
            "artifact_path": input.output_path,
            "rows_plotted": batches.iter().map(|b| b.num_rows()).sum::<usize>(),
        });

        Ok(ToolResult::success_json(content))
    }
}

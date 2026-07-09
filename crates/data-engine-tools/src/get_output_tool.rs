use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;
use arrow::util::pretty::pretty_format_batches;
use arrow_array::{Array, RecordBatch, StringArray};
use datafusion::prelude::DataFrame;

use crate::ExecError;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

#[tool(
    name = "get_output",
    description = "Get the output DataFrames of a node after a DAG run. \
                  The node must have been executed (status Success). \
                  Returns the actual data rows, with optional offset and limit \
                  to control pagination. If the DataFrame is large, use limit \
                  to avoid returning excessive data."
)]
pub struct GetOutputInput {
    /// The node id to query output for.
    pub id: String,
    /// Number of rows to skip from the beginning. Defaults to 0.
    pub offset: Option<usize>,
    /// Maximum number of rows to return. Defaults to 100. Set to 0 for unlimited.
    pub limit: Option<usize>,
}

pub struct GetOutputTool {
    client: Arc<DataEngineClient>,
}

impl GetOutputTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

/// Convert a scalar value from an Arrow array at the given row index into a
/// `serde_json::Value`. Covers the common types encountered in bioinformatics
/// and tabular data.
fn column_value(col: &dyn Array, row: usize) -> serde_json::Value {
    if col.is_null(row) {
        return serde_json::Value::Null;
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::Int8Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::Int16Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::Int32Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::Int64Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::UInt8Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::UInt16Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::UInt32Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::UInt64Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::Float32Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::Float64Array>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::BooleanArray>() {
        return serde_json::json!(c.value(row));
    }
    // String / Utf8 / LargeUtf8
    if let Some(c) = col.as_any().downcast_ref::<StringArray>() {
        return serde_json::json!(c.value(row));
    }
    if let Some(c) = col.as_any().downcast_ref::<arrow_array::LargeStringArray>() {
        return serde_json::json!(c.value(row));
    }
    // Fallback: JSON representation of the datatype
    serde_json::json!({"_type": col.data_type().to_string()})
}

/// Convert collected `RecordBatch`es into a JSON array of row objects.
fn batches_to_rows(batches: &[RecordBatch]) -> serde_json::Value {
    let mut rows = Vec::new();
    for batch in batches {
        for row_idx in 0..batch.num_rows() {
            let mut row = serde_json::Map::new();
            for (col_idx, field) in batch.schema().fields().iter().enumerate() {
                let val = column_value(batch.column(col_idx).as_ref(), row_idx);
                row.insert(field.name().clone(), val);
            }
            rows.push(serde_json::Value::Object(row));
        }
    }
    serde_json::Value::Array(rows)
}

#[async_trait]
impl ToolFunction for GetOutputTool {
    type Input = GetOutputInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let Some(dfs) = self
            .client
            .get_output(input.id.clone())
            .await
            .map_err(ExecError::from)?
        else {
            return Ok(ToolResult::error(format!(
                "no output found for node '{}'",
                input.id
            )));
        };

        let offset = input.offset.unwrap_or(0);
        let default_limit: usize = 100;
        let limit = input.limit.unwrap_or(default_limit);
        let unlimited = limit == 0;

        let mut outputs_info = Vec::with_capacity(dfs.len());
        for (name, df) in dfs.iter() {
            let total_rows = df.clone().count().await.unwrap_or(0);

            let fields: Vec<serde_json::Value> = df
                .schema()
                .fields()
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "name": f.name(),
                        "type": f.data_type().to_string(),
                    })
                })
                .collect();

            // Collect actual data rows with offset + limit.
            // DataFrame::limit(skip, fetch) — skip acts as offset, fetch caps rows.
            let fetch = if unlimited { None } else { Some(limit) };
            let queried =
                df.clone()
                    .limit(offset, fetch)
                    .map_err(|e| ToolError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
            let batches = queried.collect().await.unwrap_or_default();
            // let rows = batches_to_rows(&batches);
            let rows = pretty_format_batches(&batches)
                .map_err(|e| ToolError::ExecutionFailed {
                    source: Box::new(e),
                })?
                .to_string();
            let returned_rows: usize = batches.iter().map(|b| b.num_rows()).sum();

            let entry = serde_json::json!({
                "name": name,
                "columns": fields.len(),
                "total_rows": total_rows,
                "returned_rows": returned_rows,
                "offset": offset,
                "limit": if unlimited { serde_json::Value::String("unlimited".into()) } else { serde_json::json!(limit) },
                "fields": fields,
                "data": rows,
            });

            outputs_info.push(entry);
        }

        let content = serde_json::json!({
            "node": input.id,
            "output_count": outputs_info.len(),
            "outputs": outputs_info,
        });

        Ok(ToolResult::success_json(content))
    }
}

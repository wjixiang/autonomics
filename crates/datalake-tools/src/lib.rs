use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::default_retry;
use async_trait::async_trait;
use datalake::datalake::Datalake;
use iceberg::{Catalog, NamespaceIdent};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, agentik_proc::ToolInput)]
#[tool(
    name = "iceberg_list_namespaces",
    description = "List namespaces under the given ident path(attention: this tool do not return all nested namespaces)"
)]
pub struct IcebergListNamespacesInput {
    #[desc = "Iceberg namespace path, e.g. 'warehouse.analytics'"]
    ident: String,
}

pub struct IcebergListNamespaceTool {}

#[async_trait]
impl ToolFunction for IcebergListNamespaceTool {
    #[doc = " Strongly-typed input parameter struct. See trait docs."]
    #[doc = ""]
    #[doc = " The `ToolInput` bound means `Input` can describe its own"]
    #[doc = " [`ToolDefinition`] (typically derived via `#[derive(ToolInput)]`),"]
    #[doc = " which lets `definition()` delegate automatically."]
    type Input = IcebergListNamespacesInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let datalake = Datalake::default();
        let catalog = datalake.get_catalog().await?;
        let result = catalog
            .list_namespaces(Some(&NamespaceIdent::from_strs(&[input.ident]).unwrap()))
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                source: Box::new(e),
            })?;

        Ok(ToolResult::success(format!("{:?}", result)))
    }
}

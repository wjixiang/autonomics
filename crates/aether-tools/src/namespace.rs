//! Namespace-level Iceberg tools: list, create, check existence, drop.
//!
//! All tools route through the shared [`AetherWorkspace`] catalog so that
//! the REST connection is reused across invocations.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use datalake::aether::AetherWorkspace;
use iceberg::Catalog;
use serde::{Deserialize, Serialize};

use crate::common::{err, parse_namespace};
use crate::ns_to_json;

// --- list namespaces --------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "iceberg_list_namespaces",
    description = "List Iceberg namespaces directly under a parent namespace. Returns only the immediate children (one level), not the full nested tree. Omit `parent` to list top-level namespaces."
)]
pub struct IcebergListNamespacesInput {
    #[desc = "Parent namespace path (dotted), e.g. 'warehouse.analytics'. Omit to list top-level namespaces."]
    pub parent: Option<String>,
}

pub struct IcebergListNamespacesTool {
    pub workspace: Arc<AetherWorkspace>,
}

#[async_trait]
impl ToolFunction for IcebergListNamespacesTool {
    type Input = IcebergListNamespacesInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let catalog = self.workspace.catalog().await.map_err(err)?;

        let parent = match input.parent.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(p) => Some(parse_namespace(p).map_err(err)?),
            None => None,
        };

        let namespaces = catalog
            .list_namespaces(parent.as_ref())
            .await
            .map_err(err)?;

        let rows: Vec<serde_json::Value> = namespaces
            .iter()
            .map(|ns| serde_json::json!({ "namespace": ns.as_ref().join(".") }))
            .collect();

        Ok(ToolResult::success_json(serde_json::json!({
            "parent": input.parent,
            "namespaces": rows,
            "count": rows.len(),
        })))
    }
}

// --- create namespace -------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "iceberg_create_namespace",
    description = "Create an Iceberg namespace. By default fails if the namespace already exists; set `if_not_exists` to true to make it idempotent."
)]
pub struct IcebergCreateNamespaceInput {
    #[desc = "Namespace path (dotted) to create, e.g. 'warehouse.analytics'"]
    pub namespace: String,
    #[desc = "If true, succeed (returning the existing namespace) when it already exists. Defaults to false."]
    pub if_not_exists: Option<bool>,
}

pub struct IcebergCreateNamespaceTool {
    pub workspace: Arc<AetherWorkspace>,
}

#[async_trait]
impl ToolFunction for IcebergCreateNamespaceTool {
    type Input = IcebergCreateNamespaceInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let catalog = self.workspace.catalog().await.map_err(err)?;
        let namespace = parse_namespace(&input.namespace).map_err(err)?;
        let exists = catalog.namespace_exists(&namespace).await.map_err(err)?;

        if exists && input.if_not_exists.unwrap_or(false) {
            let ns = catalog.get_namespace(&namespace).await.map_err(err)?;
            return Ok(ToolResult::success_json(ns_to_json(&ns, true)));
        }

        let ns = catalog
            .create_namespace(&namespace, std::collections::HashMap::new())
            .await
            .map_err(err)?;

        Ok(ToolResult::success_json(ns_to_json(&ns, false)))
    }
}

// --- namespace exists -------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "iceberg_namespace_exists",
    description = "Check whether an Iceberg namespace exists. Returns a JSON object with an `exists` boolean."
)]
pub struct IcebergNamespaceExistsInput {
    #[desc = "Namespace path (dotted) to check, e.g. 'warehouse.analytics'"]
    pub namespace: String,
}

pub struct IcebergNamespaceExistsTool {
    pub workspace: Arc<AetherWorkspace>,
}

#[async_trait]
impl ToolFunction for IcebergNamespaceExistsTool {
    type Input = IcebergNamespaceExistsInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let catalog = self.workspace.catalog().await.map_err(err)?;
        let namespace = parse_namespace(&input.namespace).map_err(err)?;
        let exists = catalog.namespace_exists(&namespace).await.map_err(err)?;
        Ok(ToolResult::success_json(serde_json::json!({
            "namespace": input.namespace,
            "exists": exists,
        })))
    }
}

// --- drop namespace ---------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "iceberg_drop_namespace",
    description = "Drop (delete) an Iceberg namespace. The namespace must be empty (contain no tables) for the catalog to allow this."
)]
pub struct IcebergDropNamespaceInput {
    #[desc = "Namespace path (dotted) to drop, e.g. 'warehouse.analytics'"]
    pub namespace: String,
}

pub struct IcebergDropNamespaceTool {
    pub workspace: Arc<AetherWorkspace>,
}

#[async_trait]
impl ToolFunction for IcebergDropNamespaceTool {
    type Input = IcebergDropNamespaceInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let catalog = self.workspace.catalog().await.map_err(err)?;
        let namespace = parse_namespace(&input.namespace).map_err(err)?;
        catalog.drop_namespace(&namespace).await.map_err(err)?;
        Ok(ToolResult::success_json(serde_json::json!({
            "namespace": input.namespace,
            "dropped": true,
        })))
    }
}

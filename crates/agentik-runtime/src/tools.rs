//! Tool assembly functions for the default agent configuration.
//!
//! Each function returns a [`Vec<ToolRegistration>`]. Compose them
//! to build custom tool sets.

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;
use anyhow::Context;
use data_engine::data_session::DataSession;
use data_engine::DatasetStore;
use eutils_rs::EutilsClient;
use file_base::OpendalFileStorage;
use opengwas_rs::OpengwasClient;

/// Iceberg namespace/table tools + DataFusion dataset tools.
///
/// Requires async initialisation of the Aether workspace.
pub async fn iceberg_and_dataset_tools() -> anyhow::Result<Vec<ToolRegistration>> {
    let workspace = Arc::new(
        DataSession::new()
            .await
            .context("failed to initialise DataSession")?,
    );
    let store = Arc::new(DatasetStore::from_workspace(&workspace));
    let mut tools = iceberg_tools::iceberg_registrations(workspace);
    tools.extend(iceberg_tools::dataset_registrations(store));
    Ok(tools)
}

/// OpenGWAS tools (GWAS catalog lookup).
pub fn opengwas_tools(file_storage: Arc<OpendalFileStorage>) -> Vec<ToolRegistration> {
    let opengwas = Arc::new(OpengwasClient::new(None));
    opengwas_rs::opengwas_registrations(opengwas, file_storage)
}

/// NCBI E-utilities tools (PubMed, Entrez).
pub fn eutils_tools() -> Vec<ToolRegistration> {
    let eutils = Arc::new(EutilsClient::from_env());
    eutils_rs::eutils_registrations(eutils)
}

/// The complete default tool set: File + Iceberg + Dataset + OpenGWAS + E-utilities.
///
/// Pass a shared [`OpendalFileStorage`] used by both the file-base tools
/// and the OpenGWAS download tool.
pub async fn default_tool_set(
    file_storage: Arc<OpendalFileStorage>,
) -> anyhow::Result<Vec<ToolRegistration>> {
    let mut tools = file_base::file_base_registrations(file_storage.clone());
    tools.extend(iceberg_and_dataset_tools().await?);
    tools.extend(opengwas_tools(file_storage));
    tools.extend(eutils_tools());
    Ok(tools)
}

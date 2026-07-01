//! Tool assembly functions for the default agent configuration.
//!
//! Each function returns a [`Vec<ToolRegistration>`]. Compose them
//! to build custom tool sets.
//!
//! Iceberg + in-memory dataset tools previously lived here behind
//! `iceberg_and_dataset_tools`. Those crates (`iceberg-tools`,
//! `data-ingest`, plus `data-engine::dataset*`) have been removed as
//! part of the engine rewrite; only file / OpenGWAS / E-utilities
//! tools remain in the default set.

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;
use eutils_rs::EutilsClient;
use fs::OpendalFileStorage;
use opengwas_rs::OpengwasClient;

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

/// The complete default tool set: File + OpenGWAS + E-utilities.
///
/// Pass a shared [`OpendalFileStorage`] used by both the fs tools
/// and the OpenGWAS download tool.
pub fn default_tool_set(
    file_storage: Arc<OpendalFileStorage>,
) -> anyhow::Result<Vec<ToolRegistration>> {
    let mut tools = fs::file_base_registrations(file_storage.clone());
    tools.extend(opengwas_tools(file_storage));
    tools.extend(eutils_tools());
    Ok(tools)
}

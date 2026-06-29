//! Agent tools for data ingestion.

pub mod ingest_csv;
pub mod ingest_vcf;

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;
use data_engine::DatasetStore;

/// Create tool registrations for all data-ingest agent tools.
pub fn data_ingest_registrations(store: Arc<DatasetStore>) -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(ingest_csv::IngestCsvTool { store: store.clone() }),
        ToolRegistration::from(ingest_vcf::IngestVcfTool { store }),
    ]
}

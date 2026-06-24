//! Agent tool layer wrapping the E-utilities SDK.
//!
//! Each tool maps to one or more E-utility methods. Wire them into an agent's
//! toolset via [`eutils_registrations`].

mod egquery;
mod einfo;
mod efetch;
mod elink;
mod espell;
mod esearch;
mod esummary;

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;

pub(crate) use self::helpers::json_err;
use crate::EutilsClient;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

mod helpers {
    use agentik_core::tools::ToolError;

    pub(crate) fn json_err(e: anyhow::Error) -> ToolError {
        e.into()
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Build [`ToolRegistration`]s for all E-utility tools.
///
/// Pass a shared [`EutilsClient`] so every tool reuses the same HTTP
/// connection pool.
pub fn eutils_registrations(client: Arc<EutilsClient>) -> Vec<ToolRegistration> {
    use agentik_core::tools::ToolRegistration as R;
    vec![
        R::from(esearch::PubmedSearchTool {
            client: client.clone(),
        }),
        R::from(efetch::PubmedFetchTool {
            client: client.clone(),
        }),
        R::from(esummary::PubmedSummaryTool {
            client: client.clone(),
        }),
        R::from(elink::PubmedRelatedTool {
            client: client.clone(),
        }),
        R::from(espell::PubmedSpellTool {
            client: client.clone(),
        }),
        R::from(einfo::EInfoTool {
            client: client.clone(),
        }),
        R::from(egquery::EGQueryTool { client }),
    ]
}

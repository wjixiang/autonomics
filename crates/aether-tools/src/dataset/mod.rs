//! Dataset agent tools: in-memory analytical datasets backed by Iceberg.
//!
//! These tools wrap [`datalake::DatasetStore`] and follow the same
//! `ToolFunction` pattern as the Iceberg CRUD tools. Wire them into an agent's
//! toolset via [`dataset_registrations`].
//!
//! # Layers
//!
//! | Layer | Tools | Effect on store |
//! |-------|-------|-----------------|
//! | **L1 ingestion** | `dataset_load_table` | registers a new dataset |
//! | **L2 inspection** | `dataset_list`, `dataset_describe`, `dataset_preview`, `dataset_drop` | read-only, except `drop` |
//! | **L3 transform** | `dataset_select`, `dataset_sort`, `dataset_limit`, `dataset_union`, `dataset_sql` | modifies datasets |
//! | **L4 analysis** | `dataset_summarize`, `dataset_ols`, `dataset_ivw`, `dataset_egger` | creates result datasets |
//!
//! Iceberg direct peek is provided by `iceberg_preview_table` (in the Iceberg
//! tool layer), not as a dataset tool — peeking an Iceberg table does not
//! interact with the in-memory store.

pub mod describe;
pub mod drop;
pub mod egger;
pub mod ivw;
pub mod limit;
pub mod list;
pub mod load_table;
pub mod map;
pub mod ols;
pub mod preview;
pub mod select;
pub mod sort;
pub mod sql;
pub mod summarize;
pub mod union;

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;
use datalake::DatasetStore;

pub use describe::{DatasetDescribeInput, DatasetDescribeTool};
pub use drop::{DatasetDropInput, DatasetDropTool};
pub use egger::{DatasetEggerInput, DatasetEggerTool};
pub use ivw::{DatasetIvwInput, DatasetIvwTool};
pub use limit::{DatasetLimitInput, DatasetLimitTool};
pub use list::{DatasetListInput, DatasetListTool};
pub use load_table::{DatasetLoadTableInput, DatasetLoadTableTool};
pub use map::{DatasetMapInput, DatasetMapTool};
pub use ols::{DatasetOlsInput, DatasetOlsTool};
pub use preview::{DatasetPreviewInput, DatasetPreviewTool};
pub use select::{DatasetSelectInput, DatasetSelectTool};
pub use sort::{DatasetSortInput, DatasetSortTool};
pub use sql::{DatasetSqlInput, DatasetSqlTool};
pub use summarize::{DatasetSummarizeInput, DatasetSummarizeTool};
pub use union::{DatasetUnionInput, DatasetUnionTool};

/// All dataset tool registrations, ready to register into a toolset.
///
/// Currently provides 15 tools across four layers:
/// - **Ingestion** (1): `dataset_load_table`
/// - **Inspection** (4): `dataset_list`, `dataset_describe`, `dataset_preview`,
///   `dataset_drop`
/// - **Transform** (6): `dataset_select`, `dataset_sort`, `dataset_limit`,
///   `dataset_union`, `dataset_sql`, `dataset_map`
/// - **Analysis** (4): `dataset_summarize`, `dataset_ols`, `dataset_ivw`,
///   `dataset_egger`
///
/// Iceberg direct preview is provided by `iceberg_preview_table` in the Iceberg
/// tool layer (see [`crate::iceberg_registrations`]).
pub fn dataset_registrations(store: Arc<DatasetStore>) -> Vec<ToolRegistration> {
    vec![
        // L1 — ingestion
        ToolRegistration::from(DatasetLoadTableTool { store: store.clone() }),
        // L2 — inspection (read-only, plus drop)
        ToolRegistration::from(DatasetListTool { store: store.clone() }),
        ToolRegistration::from(DatasetDescribeTool { store: store.clone() }),
        ToolRegistration::from(DatasetPreviewTool { store: store.clone() }),
        ToolRegistration::from(DatasetDropTool { store: store.clone() }),
        // L3 — transform
        ToolRegistration::from(DatasetSelectTool { store: store.clone() }),
        ToolRegistration::from(DatasetSortTool { store: store.clone() }),
        ToolRegistration::from(DatasetLimitTool { store: store.clone() }),
        ToolRegistration::from(DatasetUnionTool { store: store.clone() }),
        ToolRegistration::from(DatasetSqlTool { store: store.clone() }),
        ToolRegistration::from(DatasetMapTool { store: store.clone() }),
        // L4 — analysis
        ToolRegistration::from(DatasetSummarizeTool { store: store.clone() }),
        ToolRegistration::from(DatasetOlsTool { store: store.clone() }),
        ToolRegistration::from(DatasetIvwTool { store: store.clone() }),
        ToolRegistration::from(DatasetEggerTool { store }),
    ]
}

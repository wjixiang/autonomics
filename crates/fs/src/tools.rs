//! Agent tool layer wrapping [`OpendalFileStorage`].
//!
//! Wire into an agent's toolset via [`file_base_registrations`].

mod file_delete;
mod file_edit;
mod file_info;
mod file_list;
mod file_read;
mod file_write;

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;

use crate::storage::OpendalFileStorage;

/// Build [`ToolRegistration`]s for all file system tools.
///
/// Pass a shared [`OpendalFileStorage`] so every tool reuses the same
/// OpenDAL operator.
pub fn file_base_registrations(storage: Arc<OpendalFileStorage>) -> Vec<ToolRegistration> {
    use agentik_core::tools::ToolRegistration as R;
    vec![
        R::from(file_read::FileReadTool { storage: storage.clone() }),
        R::from(file_write::FileWriteTool { storage: storage.clone() }),
        R::from(file_edit::FileEditTool { storage: storage.clone() }),
        R::from(file_list::FileListTool { storage: storage.clone() }),
        R::from(file_delete::FileDeleteTool { storage: storage.clone() }),
        R::from(file_info::FileInfoTool { storage }),
    ]
}

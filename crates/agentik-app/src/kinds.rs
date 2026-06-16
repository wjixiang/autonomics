//! Agent kind registrations for the host binary.
//!
//! Each struct here implements [`agentik_runtime::AgentKindFactory`]
//! and is registered into the runtime registry at startup.

use std::sync::Arc;

use agentik_core::context::AgentContext;
use agentik_core::tools::{ToolRegistration, Toolset};
use agentik_runtime::registry::{AgentKindError, AgentKindFactory};

/// A generic "coder" agent kind that uses the built-in core tools
/// (bash, read, write, edit, glob, grep, webfetch) and an in-memory
/// context store.
pub struct GenericCoderKind;

impl GenericCoderKind {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl AgentKindFactory for GenericCoderKind {
    fn name(&self) -> &str {
        "coder"
    }

    fn display_name(&self) -> &str {
        "Generic Coder"
    }

    async fn build_context(&self) -> Result<Arc<dyn AgentContext>, AgentKindError> {
        Ok(Arc::new(agentik_core::context::InMemoryAgentContext::new()))
    }

    fn build_tools(&self) -> Vec<ToolRegistration> {
        agentik_tools::primitive_registrations()
    }

    fn default_identity(&self) -> Option<&str> {
        Some("You are a helpful coding assistant. You can read, write, and edit files, run shell commands, and fetch web content.")
    }
}

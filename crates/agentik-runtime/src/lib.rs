//! Sync-to-async bridge for running an agentik agent from a sync context.
//!
//! [`AgentRuntime`] owns a sender for [`InternalEvent`]. The agent is
//! spawned once inside [`new`] and communicates exclusively through
//! channels — no `Arc<Mutex<Agent>>` needed.
//!
//! The caller is responsible for keeping the tokio runtime alive for
//! the lifetime of this struct.

pub mod tools;
pub mod runtime;

pub use runtime::AgentRuntime;
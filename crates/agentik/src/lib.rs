//! # agentik
//!
//! Umbrella crate for the **agentik** ecosystem. Re-exports the public API
//! of [`agentik_core`], [`agentik_sdk`], and [`agentik_types`] under a
//! single dependency so downstream users only need `cargo add agentik`.
//!
//! ## Layout
//!
//! - [`core`] — the agent runtime: `Agent`, `AgentBuilder`, contexts, tools,
//!   memory, storage, lifecycle, process, prompt, testing.
//! - [`sdk`] — the Anthropic SDK: `Anthropic` client, config, files,
//!   streaming, retries, models, providers, resources, tokens.
//! - [`types`] — shared wire types: `Message`, `Tool`, errors, models,
//!   batches, streaming events, files, agent events.
//!
//! ## Quick start
//!
//! ```no_run
//! use agentik::{Agent, Anthropic, Message};
//! ```

// -------------------------------------------------------------------
// Sub-namespaces — wholesale re-export so that
// `agentik::core::AgentBuilder`, `agentik::sdk::MessageStream`,
// `agentik::types::Message` all resolve.
// -------------------------------------------------------------------
pub mod core {
    pub use agentik_core::*;
}

pub mod sdk {
    pub use agentik_sdk::*;
}

pub mod types {
    pub use agentik_types::*;
}

// -------------------------------------------------------------------
// Curated top-level re-exports — the most common types, lifted to
// the umbrella root for ergonomic single-import usage.
// Keep this list short. If it grows past ~20 entries, demote something
// to its sub-namespace instead.
// -------------------------------------------------------------------

// From agentik-core
pub use agentik_core::{
    agent_builder::AgentBuilder,
    context::{AgentContext, ContextSnapshot, InMemoryAgentContext},
    error::AgentError,
    Agent,
};

// From agentik-sdk
pub use agentik_sdk::{Anthropic, ClientConfig, Error as SdkError, MessageStream};

// From agentik-types
pub use agentik_types::{ContentBlock, Message, Role, Tool, Usage};

// -------------------------------------------------------------------
// Macros — `#[macro_export]` macros are re-exported with a plain
// `pub use` so downstream code can write `use agentik::tool_function;`.
// -------------------------------------------------------------------
pub use agentik_core::tool_function;

# agentik-core

Domain-agnostic agent runtime. Plug in any `AgentContext` to specialize.

## Sibling submodules

This crate is published as a single package. Its sibling submodules
provide shared infrastructure and are pulled in via git:

- `agentik-types` — Shared type definitions (LLM API surface, message types)
- `agentik-sdk` — LLM provider SDKs (Anthropic, Mimo, etc.)

## Extension points

The `AgentContext` trait (in `agentik-core`) is the seam between the
runtime and any domain. Implement it to plug in your own:

- `on_startup_location()` — initial state snapshot
- `on_startup_diagnostics()` — pre-flight issues
- `take_snapshot()` / `on_snapshot_change()` — state drift detection
- `is_mutation_tool()` — which tools change state
- `on_mutation_diagnostics()` — post-mutation issues
- `system_prompt_section()` — domain knowledge for the LLM
- `tool_registrations()` — domain-specific tools

## Usage from another project

```toml
# Cargo.toml
[dependencies]
agentik-core = { git = "https://github.com/yourorg/agentik-core", tag = "v0.1.0" }
```

```rust
use agentik_core::{Agent, AgentContext, ContextSnapshot};
use std::sync::Arc;

struct MyContext { /* ... */ }

#[async_trait::async_trait]
impl AgentContext for MyContext { /* ... */ }

let ctx: Arc<dyn AgentContext> = Arc::new(MyContext::new());
let agent = Agent::builder()
    .with_model_pool(pool)
    .with_context(ctx)
    .build()
    .await?;
```

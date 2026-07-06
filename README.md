# agentik

A Rust workspace for building LLM agents on top of the Anthropic-compatible API surface. It ships a type-safe SDK, a proc-macro for declarative tool schemas, and a domain-agnostic agent runtime with a multi-agent process manager.

This project is a hard fork of [dimichgh/anthropic-sdk-rust](https://github.com/dimichgh/anthropic-sdk-rust), extended with an agent framework on top.

## Workspace

`agentik` is a Cargo workspace containing four crates:

| Crate                                   | Description                                                                                                                                                 |
| --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`agentik-types`](crates/agentik-types) | Shared type definitions for the Anthropic API — messages, tools, batches, files, models, streaming events, agent events, errors.                            |
| [`agentik-sdk`](crates/agentik-sdk)     | Full-featured async client: HTTP layer, SSE streaming engine, retry/backoff, multi-provider abstraction, model pool, token counter, file utilities.         |
| [`agentik-proc`](crates/agentik-proc)   | Proc-macro crate. Provides `#[derive(ToolInput)]` so tool input structs auto-generate their own JSON Schema `Tool` definition.                              |
| [`agentik-core`](crates/agentik-core)   | Domain-agnostic agent runtime. Plug in any `AgentContext` to specialize behavior: agent loop, memory with compaction, lifecycle, toolset, `ProcessManager`. |

```
crates/
├── agentik-types/   # Shared types (no deps on the rest of the workspace)
├── agentik-sdk/     # Async client + provider abstraction (depends on -types)
├── agentik-proc/    # #[derive(ToolInput)] (depends on -sdk at expansion site)
└── agentik-core/    # Agent runtime (depends on -sdk and -proc)
```

## Features

### SDK (`agentik-sdk`)

- **Messages API** — Create conversations with system prompts, multi-turn, temperature, top-p, stop sequences
- **Streaming (SSE)** — Token-by-token streaming with event-driven callbacks (`on_text`, `on_message`, `on_error`, `on_end`) and async iteration via the `Stream` trait
- **Stream reliability** — Automatic idle-timeout detection and reconnection before `MessageStart`, graceful HTTP body draining, configurable retry policies with exponential backoff and jitter
- **Tool / function calling** — JSON Schema tools, `tool_use` / `tool_result` blocks, server tools (web search)
- **Vision** — Send images via base64 or URL
- **Files API (Beta)** — Upload, list, download with SHA-256 integrity verification
- **Batch processing (Beta)** — Create and manage batch inference requests
- **Models API** — List and inspect models with capability and pricing metadata
- **Token & cost tracking** — `TokenCounter` with per-model pricing, accumulated usage, and cost estimation
- **Multi-provider abstraction** — `LlmProvider` trait with implementations:
  - Anthropic (direct)
  - DeepSeek (`deepseek-v4-pro`, `deepseek-v4-flash`)
  - MiniMax
  - SenseNova
  - Mimo
  - ZAI
- **Model pool** — Round-robin model selection across providers, with sticky selection by name
- **Flexible auth** — Anthropic `x-api-key`, Bearer token, or custom header for third-party gateways
- **Mock support** — `MockApiClient` via `mockall` for testing

### Agent runtime (`agentik-core`)

- **Uniform agent loop** — One behavioral loop for all agents. Agent personality and tooling are configured _only_ through the toolset and system prompt; no agent-specific code paths in the loop itself (see `crates/agentik-core/src/agent.rs`).
- **Reactive context** — `AgentContext` trait: implement `read()` / `write()`. The loop polls the version at each boundary and injects a `[context-update]` message into memory when it changes. Built-in `InMemoryAgentContext` for tests.
- **Memory with compaction** — `Memory` keeps a rolling list of summarized `MemoryItem`s. When token pressure rises against the model's `context_length`, the oldest segment is summarized by the LLM into a `summary` and a fresh segment is opened.
- **Toolset** — `ToolRegistration` + `Toolset` handle schema exposure, parallel dispatch, and per-tool timeouts. Every `T: ToolFunction` is auto-erased to `DynToolFunction` for heterogeneous storage.
- **Built-in tools** — `attempt_complete`, `abort_task` (lifecycle), `bash` (subprocess with kill-on-drop and tail-truncated output).
- **Lifecycle** — `AgentLifecycle` (IDLE / RUNNING / ABORTED) driven by built-in lifecycle tools, so agents self-terminate without external orchestration.
- **Retry with feedback** — Retryable `AgentError`s trigger exponential backoff and the failure reason is injected back into memory for the next attempt.
- **Observation** — Optional `mpsc` event channel streams `AgentUiEvent`s (Thinking, LlmResponse, ToolCall, ToolResult, Requesting, Done, Error) to a TUI or logger.
- **Snapshots** — `AgentSnapshotStorage` trait with a SQLite backend for persisting agent memory and status.
- **Multi-agent `ProcessManager`** — Spawn, start, stop, restart, and inject messages into multiple agents as independent tokio tasks; aggregates all per-agent events into one `broadcast::Receiver<ProcessEvent>` stream with exit status (`Completed` / `Error` / `Panicked` / `Cancelled` / `Stopped`).

### Proc macros (`agentik-proc`)

- **`#[derive(ToolInput)]`** — Generates `impl ToolInput` from a struct, including the `ToolBuilder` chain, required vs. optional fields (via `Option<T>` or `#[default = ...]`), and `#[desc = "..."]` per-field descriptions. Pair with `#[tool(name = "...", description = "...")]` on the struct.

## Quick Start

### Talking to a model directly via the SDK

```toml
[dependencies]
agentik-sdk = "0.3"
```

```rust
use agentik_sdk::Anthropic;
use agentik_types::MessageCreateBuilder;

#[tokio::main]
async fn main() -> agentik_sdk::Result<()> {
    let client = Anthropic::new("your-api-key", "https://api.anthropic.com")?;

    let message = client.messages().create(
        MessageCreateBuilder::new("claude-sonnet-4-20250514", 1024)
            .system("You are a helpful assistant.")
            .user("Hello, Claude!")
            .build(),
    ).await?;

    println!("Response: {:?}", message.content);
    Ok(())
}
```

### Running an agent with a custom tool

```toml
[dependencies]
agentik-core = { path = "..." }
agentik-sdk  = { path = "..." }
agentik-proc = { path = "..." }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use std::sync::Arc;

use agentik_core::agent::{Agent, AgentConfig};
use agentik_core::context::InMemoryAgentContext;
use agentik_core::tools::{ToolFunction, ToolResult, ToolRegistration, error::ToolError};
use agentik_core::toolset::Toolset;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::provider::mimo::{MimoProvider, MODEL_MIMO_V2_5};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// Declarative tool schema via proc macro.
#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(name = "echo", description = "Echo back a message")]
pub struct EchoInput {
    #[desc = "The text to echo back"]
    pub text: String,
}

pub struct EchoTool;

#[async_trait]
impl ToolFunction for EchoTool {
    type Input = EchoInput;

    async fn run(&self, input: EchoInput) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::success("echo", format!("echo: {}", input.text)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = MimoProvider::new(None, std::env::var("MIMO_API_KEY")?);
    let model = provider.get_model(MODEL_MIMO_V2_5)?;

    let mut pool = ModelPool::new();
    pool.add_model(model);

    let ctx = Arc::new(InMemoryAgentContext::new());

    let mut agent = Agent::builder()
        .with_model_pool(Arc::new(pool))
        .with_context(ctx)
        .with_system_prompt_identity("You are a minimal demo agent.")
        .with_config(AgentConfig::default())
        .build()
        .await?;

    agent.register_tool(ToolRegistration::from(EchoTool))?;
    agent.start().await?;
    Ok(())
}
```

### Multi-agent orchestration

```rust
use agentik_core::process::ProcessManager;

let manager = ProcessManager::new();

// Spawn (registers but does not start) — returns the agent ID.
let id = manager.spawn(builder).await?;

manager.start(&id)?;
manager.inject_message(&id, vec![/* ContentBlock::Text { ... } */])?;

let mut events = manager.events();
while let Ok(ev) = events.recv().await {
    println!("{ev:?}");
}

// Graceful shutdown of every agent.
let exits = manager.shutdown().await;
```

## Tool authoring

Tools implement `ToolFunction` with an associated strongly-typed `Input`. The framework deserializes the LLM's JSON into `Input` before `run` is called, so tool bodies stay free of `serde_json::Value` plumbing.

```rust
#[derive(Deserialize, agentik_proc::ToolInput)]
#[tool(name = "get_weather", description = "Current weather for a city")]
struct WeatherInput {
    #[desc = "City name"]
    city: String,

    #[desc = "Units: metric or imperial"]
    #[default = "metric"]
    units: Option<String>,
}

struct WeatherTool;

#[async_trait]
impl ToolFunction for WeatherTool {
    type Input = WeatherInput;
    async fn run(&self, i: WeatherInput) -> Result<ToolResult, ToolError> {
        // ... fetch weather ...
        Ok(ToolResult::success("weather", format!("{}: sunny", i.city)))
    }
}
```

The built-in lifecycle tools (`attempt_complete`, `abort_task`) drive agent state transitions (`IDLE` / `ABORTED`).

## Configuration

```rust
use agentik_sdk::{Anthropic, ClientConfig, LogLevel, AuthMethod};
use std::time::Duration;

let config = ClientConfig::new("your-api-key", "https://api.anthropic.com")
    .with_timeout(Duration::from_secs(120))
    .with_max_retries(3)
    .with_log_level(LogLevel::Info)
    .with_auth_method(AuthMethod::Anthropic);

let client = Anthropic::with_config(config)?;
```

### Environment variables

Copy `.env.example` to `.env`:

```env
ANTHROPIC_API_KEY="your-api-key-here"
DEEPSEEK_API_KEY="your-api-key-here"
SENSENOVA_API_KEY="your-api-key-here"
ZAI_API_KEY="your-api-key-here"
MIMO_API_KEY="your-api-key-here"
```

## API Resources (SDK)

| Resource            | Description                             |
| ------------------- | --------------------------------------- |
| `client.messages()` | Create messages and streaming responses |
| `client.batches()`  | Manage batch inference requests         |
| `client.files()`    | Upload and manage files                 |
| `client.models()`   | List and inspect models                 |

## Architecture notes

- **Agent loop design.** The core loop only provides generic capabilities — request/response cycling, lifecycle management, effect application, memory compaction. It never encodes agent-specific behavior, tool selection, or prompt engineering. Configure those exclusively via the toolset and system prompt (`agent.rs:1` documents this contract).
- **Termination.** A response with no tool calls signals completion (matches the model's trained prior). `attempt_complete` is retained for compatibility but deprecated; the loop flips to `IDLE` on the no-tool-call branch regardless.
- **Type erasure.** `ToolFunction::Input` is an associated type, so heterogeneous storage erases to `DynToolFunction` via a blanket impl — concrete call sites keep full type information.
- **Multi-agent.** Each agent runs in its own tokio task with its own command channel, status watch, and cancellation token. A forwarder task merges per-agent `AgentUiEvent`s, lifecycle changes, and task-exit signals into the manager's `ProcessEvent` broadcast stream.

## Workspace structure

```
agentik/
├── Cargo.toml                          # Workspace manifest
└── crates/
    ├── agentik-types/                  # Shared type definitions
    │   └── src/
    │       ├── messages.rs             # Message, Role, ContentBlock, Builder
    │       ├── tools.rs                # Tool, ToolChoice, ToolUse, ToolResult, ToolInput
    │       ├── models.rs               # Model enum (Anthropic, Google, etc.)
    │       ├── models_api.rs           # ModelObject, ModelList, pricing, capabilities
    │       ├── streaming.rs            # SSE event types
    │       ├── batches.rs              # Batch types
    │       ├── files_api.rs            # File types
    │       ├── agent_events.rs         # AgentEvent / AgentUiEvent
    │       ├── shared.rs               # RequestId, Usage
    │       └── errors.rs               # AnthropicError
    ├── agentik-sdk/                    # Full SDK implementation
    │   └── src/
    │       ├── client.rs               # Anthropic client (entry point)
    │       ├── config.rs               # ClientConfig, LogLevel
    │       ├── http/                   # HTTP client, auth, retry, SSE parser
    │       ├── streaming.rs            # MessageStream (events + Stream trait)
    │       ├── resources/              # messages, batches, files, models APIs
    │       ├── provider/               # deepseek, minimax, sensenova, mimo, zai, ApiClient
    │       ├── model/                  # Model, ModelInfo, ModelPool
    │       ├── tokens.rs               # TokenCounter, pricing, cost tracking
    │       └── files.rs                # File, FileBuilder, integrity checks
    ├── agentik-proc/                   # Proc macros
    │   └── src/lib.rs                  # #[derive(ToolInput)]
    └── agentik-core/                   # Agent runtime
        └── src/
            ├── lib.rs
            ├── agent.rs                # Agent + agent_workflow loop
            ├── agent_builder.rs        # Fluent AgentBuilder
            ├── context.rs              # AgentContext trait, InMemoryAgentContext
            ├── lifecycle.rs            # AgentLifecycle (IDLE/RUNNING/ABORTED)
            ├── memory.rs               # Memory with summarization/compaction
            ├── message_ext.rs          # AgentMessageExt helpers
            ├── prompt/                 # SystemPromptBuilder, context, compact prompts
            ├── process/                # ProcessManager + commands + events
            ├── storage/                # AgentSnapshotStorage + SQLite impl
            ├── testing.rs              # Test helpers (dummy ModelInfo, mock pool)
            ├── toolset.rs              # Re-exports ToolRegistration / Toolset
            ├── tools/                  # ToolFunction trait, registry, executor,
            │                           #   bash_tool, lifecycle_tools, errors
            └── types.rs                # Re-exports ToolError
```

## Requirements

- Rust 1.85+ (edition 2024)

## License

MIT

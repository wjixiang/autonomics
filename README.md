# agentik-sdk

Comprehensive, type-safe Rust SDK for the Anthropic Claude API with multi-provider support, streaming, tool use, vision, file management, batch processing, and token/cost tracking.

This project is a hard fork of [dimichgh/anthropic-sdk-rust](https://github.com/dimichgh/anthropic-sdk-rust).

## Overview

`agentik-sdk` is a Rust workspace containing two crates:

| Crate | Description |
|---|---|
| [`agentik-sdk`](crates/agentik-sdk) | Full-featured async client with HTTP layer, streaming engine, retry logic, provider abstraction, model pool, token counter, and file utilities. |
| [`agentik-types`](crates/agentik-types) | Shared type definitions for the Anthropic API — messages, tools, batches, files, models, streaming events, and agent events. |

## Features

- **Messages API** — Create conversations with Claude models (system prompts, multi-turn, temperature, top-p, stop sequences)
- **Streaming (SSE)** — Real-time token-by-token streaming with event-driven callbacks (`on_text`, `on_message`, `on_error`, `on_end`) and async iteration via `Stream` trait
- **Stream reliability** — Automatic idle-timeout detection and reconnection before `MessageStart`, graceful HTTP body draining, configurable retry policies with exponential backoff and jitter
- **Tool / Function calling** — Define tools with JSON Schema, handle `tool_use` and `tool_result` content blocks, server tools (web search)
- **Vision** — Send images via base64 or URL as part of the conversation
- **Files API (Beta)** — Upload, list, and download files with integrity verification (SHA-256)
- **Batch processing (Beta)** — Create and manage batch inference requests
- **Models API** — List and inspect available models with capability and pricing metadata
- **Agent events** — Unified `AgentEvent` enum for TUI/logging integration covering the full agent lifecycle (streaming deltas, tool calls, completion)
- **Token & cost tracking** — `TokenCounter` with per-model pricing, accumulated usage stats, and cost estimation
- **Multi-provider abstraction** — `LlmProvider` trait with implementations for Anthropic-compatible providers:
  - Anthropic (direct)
  - DeepSeek (`deepseek-v4-pro`, `deepseek-v4-flash`)
  - MiniMax
  - SenseNova
  - Mimo
  - ZAI
- **Model pool** — Round-robin model selection across providers
- **Flexible auth** — Anthropic `x-api-key`, Bearer token, or custom token header for third-party gateways
- **Builder patterns** — Fluent API for `MessageCreateBuilder`, `ClientConfig`, `ToolBuilder`, `FileBuilder`, `BatchRequestBuilder`
- **Mock support** — `MockApiClient` via `mockall` for testing

## Quick Start

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

## Streaming

```rust
use agentik_sdk::Anthropic;
use agentik_types::MessageCreateBuilder;
use futures::StreamExt;

let client = Anthropic::new("your-api-key", "https://api.anthropic.com")?;

let mut stream = client.messages().create_stream(
    MessageCreateBuilder::new("claude-sonnet-4-20250514", 1024)
        .user("Tell me a story")
        .build(),
).await?;

while let Some(event) = stream.next().await {
    match event? {
        agentik_types::MessageStreamEvent::ContentBlockDelta { delta, .. } => {
            // Process incremental text
        }
        agentik_types::MessageStreamEvent::MessageStop => break,
        _ => {}
    }
}
```

Callback-based streaming is also supported:

```rust
let final_message = client.messages().create_stream(request).await?
    .on_text(|delta, snapshot| print!("{}", delta))
    .on_error(|error| eprintln!("Error: {}", error))
    .final_message().await?;
```

## Multi-Provider Setup

```rust
use agentik_sdk::provider::deepseek::{DeepseekProvider, MODEL_DEEPSEEK_V4_PRO};

let provider = DeepseekProvider::new(None, "your-deepseek-key".into());
let model = provider.get_model(MODEL_DEEPSEEK_V4_PRO)?;
```

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

### Environment Variables

Copy `.env.example` to `.env`:

```env
ANTHROPIC_API_KEY="your-api-key-here"
DEEPSEEK_API_KEY="your-api-key-here"
SENSENOVA_API_KEY="your-api-key-here"
ZAI_API_KEY="your-api-key-here"
```

## API Resources

The client exposes four resource endpoints:

| Resource | Description |
|---|---|
| `client.messages()` | Create messages and streaming responses |
| `client.batches()` | Manage batch inference requests |
| `client.files()` | Upload and manage files |
| `client.models()` | List and inspect models |

## Workspace Structure

```
agentik-sdk/
├── Cargo.toml                    # Workspace manifest
├── crates/
│   ├── agentik-types/             # Shared type definitions
│   │   └── src/
│   │       ├── messages.rs        # Message, Role, ContentBlock, Builder
│   │       ├── tools.rs           # Tool, ToolChoice, ToolUse, ToolResult
│   │       ├── models.rs          # Model enum (Anthropic, Google, etc.)
│   │       ├── models_api.rs      # ModelObject, ModelList, pricing, capabilities
│   │       ├── streaming.rs       # SSE event types
│   │       ├── batches.rs         # Batch types
│   │       ├── files_api.rs       # File types
│   │       ├── agent_events.rs    # AgentEvent enum
│   │       ├── shared.rs          # RequestId, Usage
│   │       └── errors.rs         # AnthropicError
│   └── agentik-sdk/               # Full SDK implementation
│       └── src/
│           ├── client.rs          # Anthropic client (entry point)
│           ├── config.rs          # ClientConfig, LogLevel
│           ├── http/
│           │   ├── client.rs      # HTTP client wrapper
│           │   ├── auth.rs        # AuthMethod, AuthHandler
│           │   ├── retry.rs       # RetryPolicy with backoff/jitter
│           │   └── streaming.rs  # SSE stream parser
│           ├── streaming.rs       # MessageStream (event-driven + Stream trait)
│           ├── resources/
│           │   ├── messages.rs     # Messages API
│           │   ├── batches.rs     # Batches API
│           │   ├── files.rs       # Files API
│           │   └── models.rs      # Models API
│           ├── provider/
│           │   ├── deepseek.rs    # DeepSeek provider
│           │   ├── minimax.rs     # MiniMax provider
│           │   ├── sensenova.rs  # SenseNova provider
│           │   ├── mimo.rs       # Mimo provider
│           │   ├── zai.rs        # ZAI provider
│           │   └── client.rs      # ApiClient trait
│           ├── model/             # Model, ModelInfo, ModelPool
│           ├── tokens.rs          # TokenCounter, pricing, cost tracking
│           ├── files.rs           # File, FileBuilder, integrity checks
│           └── utils.rs           # Logging init
```

## Requirements

- Rust 1.85+

## License

MIT

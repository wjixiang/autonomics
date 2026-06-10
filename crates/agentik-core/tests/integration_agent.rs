/// Minimal smoke test — verifies the SDK can parse a mimo SSE stream at all.
/// Run: MIMO_API_KEY=sk-xxx cargo test -p agentik-core --test integration_agent smoke -- --nocapture
#[tokio::test]
async fn test_sdk_mimo_stream_smoke() {
    use agentik_sdk::provider::mimo::{MimoEndpoint, TokenPlanRegion};
    use futures::StreamExt;

    let api_key = std::env::var("MIMO_API_KEY").expect("MIMO_API_KEY not set");
    let endpoint = MimoEndpoint::TokenPlan(TokenPlanRegion::China);

    let provider = agentik_sdk::provider::mimo::MimoProvider::new(Some(endpoint), api_key);

    let model = provider.get_model(MODEL_MIMO_V2_5_PRO).unwrap();

    // let params = MessageCreateBuilder::new(MODEL_MIMO_V2_5, 128)
    //     .message(Role::User, MessageContent::Text("Say hi".into()))
    //     .stream(true)
    //     .build();

    println!("sending stream request...");
    let mut stream = model
        .request_stream(vec![Message::user("hi")], &[])
        .await
        .expect("create_stream failed");

    let mut count = 0u32;
    while let Some(result) = stream.next().await {
        println!("  event: {:?}", result);
        count += 1;
    }

    let final_msg = stream.final_message().await.expect("final_message failed");
    println!("\ntotal events: {count}");
    println!("final message: {:?}", final_msg.content);
    assert!(count > 0, "expected at least one SSE event");
}

/// Integration test — calls the real Mimo streaming API.
///
/// Run with:
///   MIMO_API_KEY=sk-xxx cargo test -p agentik-core --test integration_agent -- --nocapture
///
/// Requires:
///   - `MIMO_API_KEY` env var
///   - Optionally `MIMO_ENDPOINT` (api | china | eur | sgp, default: china)
use std::sync::Arc;

use agentik_core::{
    agent::{Agent, AgentConfig},
    context::{AgentContext, ContextChanges, InMemoryAgentContext},
    message_ext::AgentMessageExt,
};
use agentik_sdk::provider::{LlmProvider, mimo::MODEL_MIMO_V2_5_PRO};
use agentik_sdk::{
    Message,
    provider::mimo::{MimoEndpoint, MimoProvider, TokenPlanRegion},
};
use agentik_sdk::{model::model_pool::ModelPool, provider::mimo::MODEL_MIMO_V2_5};
use agentik_types::AgentEvent;

// ── Helper: build an InMemoryAgentContext with initial data ──────

fn build_echo_context(prompt: &str) -> Arc<InMemoryAgentContext> {
    let ctx = InMemoryAgentContext::new();
    let mut data = std::collections::HashMap::new();
    data.insert(
        "prompt".to_string(),
        serde_json::Value::String(prompt.to_string()),
    );
    // Pre-write so version > 0, triggering injection on first loop
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        ctx.write(ContextChanges { data }).await.unwrap();
    });
    Arc::new(ctx)
}

fn build_mimo_model_pool() -> ModelPool {
    let api_key = std::env::var("MIMO_API_KEY").expect("MIMO_API_KEY not set");

    let provider = MimoProvider::new(
        Some(MimoEndpoint::TokenPlan(TokenPlanRegion::China)),
        api_key,
    );
    let model = provider
        .get_model(MODEL_MIMO_V2_5)
        .expect("failed to get mimo model");

    let mut pool = ModelPool::new();
    pool.add_model(model);
    pool
}

// ── Test ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_agent_basic_workflow_with_mimo() {
    // Arrange
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();

    let mut agent = Agent::builder()
        .with_model_pool(Arc::new(build_mimo_model_pool()))
        .with_context(build_echo_context("Say exactly: hello world"))
        .with_system_prompt_section(
            "You are a helpful assistant. Keep responses very short (one sentence).",
        )
        .with_config(AgentConfig {
            max_iterations: 3,
            max_retries: 1,
        })
        .build()
        .await
        .expect("failed to build agent");

    agent.event_tx = Some(tx);

    // Act — run agent in background, collect events
    let handle = tokio::spawn(async move {
        let _ = agent.start().await;
    });

    // Collect events with a timeout
    let events = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let mut evts = vec![];
        while let Some(e) = rx.recv().await {
            let is_terminal = matches!(e, AgentEvent::Done | AgentEvent::Error(_));
            evts.push(e);
            if is_terminal {
                break;
            }
        }
        evts
    })
    .await
    .expect("test timed out waiting for agent events");

    let _ = handle.await;

    // Print all events for manual inspection
    println!("\n=== Events received ({}) ===", events.len());
    for (i, e) in events.iter().enumerate() {
        println!("  [{i:2}] {e:?}");
    }

    // Assert
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::Done)),
        "Expected Done event, got: {:?}",
        events
            .iter()
            .map(|e| format!("{:?}", e))
            .collect::<Vec<_>>()
    );

    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::LlmResponse(_))),
        "Expected at least one LlmResponse event"
    );

    assert!(
        !events.iter().any(|e| matches!(e, AgentEvent::Error(_))),
        "Unexpected Error event"
    );
}

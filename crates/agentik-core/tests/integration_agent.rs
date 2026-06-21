use std::sync::Arc;

use agentik_core::{
    agent::{Agent, AgentConfig},
    message_ext::AgentMessageExt,
};
use agentik_sdk::Message;
use agentik_sdk::provider::mimo::MODEL_MIMO_V2_5;
use agentik_sdk::{
    model::model_pool::ModelPool,
    model::Model,
    provider::mimo::{MimoEndpoint, MimoProvider, TokenPlanRegion},
    types::AgentEvent,
};

fn build_mimo_model_pool() -> ModelPool {
    let api_key = std::env::var("MIMO_API_KEY").expect("MIMO_API_KEY not set");

    let model_info = MimoProvider::preset_models(
        api_key,
        Some(MimoEndpoint::TokenPlan(TokenPlanRegion::China)),
    )
    .into_iter()
    .find(|m| m.model_name == MODEL_MIMO_V2_5)
    .expect("preset model not found");

    let model = Model::new(model_info).expect("failed to build mimo model");

    let mut pool = ModelPool::new();
    pool.add_model(model);
    pool
}

#[tokio::test]
#[ignore]
async fn test_agent_basic_workflow_with_mimo() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();

    let mut agent = Agent::builder()
        .with_model_pool(Arc::new(build_mimo_model_pool()))
        .with_initial_messages(vec![Message::user("Say exactly: hello world")])
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
    let handle = tokio::spawn(async move {
        let _ = agent.start().await;
    });

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

    println!("\n=== Events received ({}) ===", events.len());
    for (i, e) in events.iter().enumerate() {
        println!("  [{i:2}] {e:?}");
    }

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

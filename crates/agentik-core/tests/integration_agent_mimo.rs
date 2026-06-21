/// Minimal smoke test — verifies the SDK can parse a mimo SSE stream at all.
/// Run: MIMO_API_KEY=sk-xxx cargo test -p agentik-core --test integration_agent_mimo -- --nocapture
#[tokio::test]
#[ignore]
async fn test_sdk_mimo_stream_smoke() {
    use agentik_core::message_ext::AgentMessageExt;
    use agentik_sdk::Message;
    use agentik_sdk::model::Model;
    use agentik_sdk::provider::mimo::{MODEL_MIMO_V2_5_PRO, MimoEndpoint, MimoProvider, TokenPlanRegion};
    use futures::StreamExt;

    let api_key = std::env::var("MIMO_API_KEY").expect("MIMO_API_KEY not set");
    let endpoint = MimoEndpoint::TokenPlan(TokenPlanRegion::China);

    let model_info = MimoProvider::preset_models(api_key, Some(endpoint))
        .into_iter()
        .find(|m| m.model_name == MODEL_MIMO_V2_5_PRO)
        .expect("preset model not found");

    let model = Model::new(model_info).unwrap();

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

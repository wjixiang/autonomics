//! Headless integration test: build an Agent from a SkillTree and run it.
//!
//! Requires a running skill-registry server:
//!   cargo run -p agentik-skill-server -- --skill-dir ./skills
//!
//! Then run this test (from project root):
//!   RUST_LOG=info cargo run -p agentik-app --bin test_agent

use std::path::PathBuf;
use std::sync::Arc;

use agentik_core::tools::ToolProviderRegistry;
use agentik_runtime::{AgentBlueprint, AgentEvent, ModelConfig, PoolOwner};
use agentik_sdk::types::messages::ContentBlock;

const SKILL_SERVER_ADDR: &str = "http://127.0.0.1:50051";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // 1. Connect to skill registry server.
    println!("Connecting to skill registry at {SKILL_SERVER_ADDR}...");
    let skill_client = agentik_runtime::SkillRegistryClient::connect(SKILL_SERVER_ADDR).await?;
    println!("✓ Connected to skill registry");

    // 2. Build skill tree from disk.
    let skill_tree = agentik_skill::load_skill_tree_from_dirs(&[PathBuf::from("skills")]);
    println!(
        "✓ Skill tree loaded (root: {})",
        skill_tree.root.as_ref().map(|n| n.dotpath.clone()).unwrap_or_default()
    );
    let all_tools: Vec<String> = skill_tree.collect_all_allowed_tools().into_iter().collect();
    println!("  Tools in union: {:?}", all_tools);

    // 3. Register primitive tools.
    let mut tool_provider = ToolProviderRegistry::new();
    for reg in agentik_tools::primitive_registrations() {
        tool_provider.register(reg);
    }
    println!("✓ Tool provider registered ({} tools)", tool_provider.names().len());

    // 4. Configure model pool from data/settings.json.
    let config: ModelConfig =
        serde_json::from_str(&std::fs::read_to_string("data/settings.json")?)?;
    let pool_owner = Arc::new(PoolOwner::new());
    pool_owner.configure(&config).await?;
    let model_names = pool_owner.model_names().await;
    println!("✓ Pool configured ({} models: {:?})", model_names.len(), model_names);

    // 5. Build the AgentBlueprint with skill client.
    let blueprint = AgentBlueprint::new(
        "coder",
        "Generic Coder",
        skill_tree,
        tool_provider,
    )
    .with_identity(
        "You are a helpful coding assistant. You can read, write, and edit files, \
         run shell commands, and fetch web content.",
    )
    .with_skill_client(Arc::new(tokio::sync::Mutex::new(skill_client)));

    // 6. Build the agent.
    let mut agent = blueprint.build_agent(pool_owner.current().await.unwrap()).await?;
    println!("✓ Agent built (id: {})", agent.id());

    // 7. Wire an event channel so we can observe the agent's output.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    agent.set_event_tx(event_tx);

    // Spawn a task to collect and print events.
    let event_printer = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match &event {
                AgentEvent::TextDelta(_) => {}
                AgentEvent::ThinkingDelta(_) => {}
                _ => {
                    println!("[event] {event:?}");
                }
            }
        }
    });

    // 8. Inject a user message.
    let user_msg = "Run `git status --short` and show me the output.";
    agent.inject_message(vec![ContentBlock::Text {
        text: user_msg.to_string(),
    }])?;
    println!("✓ Message injected: \"{user_msg}\"");

    // 9. Run the agent loop.
    println!("\n── Running agent ──\n");
    match agent.start().await {
        Ok(()) => println!("\n── Agent finished normally ──"),
        Err(e) => {
            println!("\n── Agent error: {e} ──");
        }
    }

    // Wait for the event printer to finish.
    drop(agent);
    let _ = event_printer.await;

    Ok(())
}

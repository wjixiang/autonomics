use std::path::PathBuf;
use std::sync::Arc;

use agentik_core::testing::get_mock_model_pool;
use agentik_core::tools::ToolProviderRegistry;
use agentik_skill::{Skill, SkillMetadata, SkillPolicy, SkillTree};
use agentik_runtime::{
    AgentBlueprint, AgentRegistry,
};

// ── Helpers ──────────────────────────────────────────────────────

/// Create a minimal Skill for testing.
fn make_root_skill() -> Skill {
    Skill {
        metadata: SkillMetadata {
            name: "root".to_string(),
            description: "Root skill for testing".to_string(),
            aliases: Vec::new(),
            when_to_use: None,
            argument_hint: None,
            user_invocable: true,
            model_invocable: true,
        },
        policy: SkillPolicy::default(),
        body: String::new(),
        references: Vec::new(),
        activation_paths: Vec::new(),
        skill_dir: PathBuf::from("/skills/root"),
    }
}

/// Build a minimal SkillTree with a single root node.
fn make_minimal_tree() -> SkillTree {
    let base = PathBuf::from("/skills");
    SkillTree::build(vec![make_root_skill()], &base)
}

/// Create a test AgentBlueprint with a minimal skill tree and empty tool provider.
fn make_test_blueprint() -> Arc<AgentBlueprint> {
    let tree = make_minimal_tree();
    let tool_provider = ToolProviderRegistry::new();
    Arc::new(
        AgentBlueprint::new("test-kind", "Test Kind", tree, tool_provider)
            .with_identity("You are a test assistant."),
    )
}

// ── Tests ─────────────────────────────────────────────────────────

#[test]
fn test_registry_crud() {
    let registry = AgentRegistry::new();

    // Initially empty
    assert!(registry.list().is_empty());

    // Register a blueprint
    let blueprint = make_test_blueprint();
    registry.register(blueprint);

    // List should contain the registered kind
    let names = registry.list();
    assert_eq!(names, vec!["test-kind".to_string()]);

    // Get by name
    let retrieved = registry.get("test-kind").expect("should find registered kind");
    assert_eq!(retrieved.name, "test-kind");
    assert_eq!(retrieved.display_name, "Test Kind");

    // Get non-existent kind returns None
    assert!(registry.get("nonexistent").is_none());

    // Unregister
    registry.unregister("test-kind");
    assert!(registry.get("test-kind").is_none());
    assert!(registry.list().is_empty());
}

#[tokio::test]
async fn test_blueprint_build_agent_from_registry() {
    let registry = AgentRegistry::new();

    // Register a blueprint
    let blueprint = make_test_blueprint();
    registry.register(blueprint);

    // Retrieve from registry
    let kind = registry
        .get("test-kind")
        .expect("should find registered kind");

    // Build agent with a mock model pool
    let model_pool = Arc::new(get_mock_model_pool("test-model"));
    let agent = kind
        .build_agent(model_pool)
        .await
        .expect("build_agent should succeed");

    // Verify agent properties
    assert_ne!(agent.id(), uuid::Uuid::nil(), "agent should have a non-nil UUID");

    // Shutdown model pool to avoid resource leaks
    drop(agent);
}

#[tokio::test]
#[ignore]
async fn test_process_manager_spawn_and_start() {
    // TODO: Configure MockApiClient to return a streaming response that triggers
    // AttemptComplete, then verify the full ProcessManager lifecycle:
    //   spawn_by_kind() → start() → collect ProcessEvent::Done → shutdown()
}

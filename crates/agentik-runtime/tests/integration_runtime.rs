//! Integration tests for [`Runtime`]: the unified portal that owns the
//! embedded skill server and the agent process manager.

use std::sync::Arc;

use agentik_runtime::{
    AgentBlueprint, AgentRegistry, ProcessManager, Runtime, RuntimeConfig,
};

use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────

/// Create a minimal SKILL.md on disk under a temporary directory.
fn write_skill_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    let skill_dir = dir.path().join("root");
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");

    let skill_md = skill_dir.join("SKILL.md");
    std::fs::write(
        &skill_md,
        "---\n\
         name: root\n\
         description: A root skill for testing\n\
         ---\n\
         \n\
         # Root Skill\n\
         \n\
         Test body.\n",
    )
    .expect("write SKILL.md");

    dir
}

// ── Tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_runtime_headless() {
    // Headless mode: no skill server, no model config.
    let runtime = Runtime::new(RuntimeConfig::headless())
        .await
        .expect("headless runtime should initialize");

    // No skill server should be running.
    assert!(
        runtime.skill_server_addr().is_none(),
        "headless mode should not start a skill server"
    );
    assert!(
        runtime.skill_client().is_none(),
        "headless mode should have no skill client"
    );

    // Registry and process manager are accessible.
    let registry: &AgentRegistry = runtime.registry();
    assert!(registry.list().is_empty());

    let _pm: &ProcessManager = runtime.process_manager();

    // Event stream is open.
    let _rx = runtime.events();

    // Graceful shutdown returns an empty result (no agents spawned).
    let results = runtime.shutdown().await;
    assert!(results.is_empty(), "no agents were spawned");
}

#[tokio::test]
async fn test_runtime_with_embedded_skill_server() {
    let skill_dirs = write_skill_dir();
    let db_dir = tempfile::tempdir().unwrap();
    let db_path = db_dir.path().join("skills.db");

    let config = RuntimeConfig::with_embedded_skill_server(vec![skill_dirs.path().to_path_buf()])
        .with_db_path(db_path);
    let runtime = Runtime::new(config)
        .await
        .expect("runtime with embedded skill server should initialize");

    // Skill server should be bound on an OS-assigned port.
    let addr = runtime
        .skill_server_addr()
        .expect("skill server should be running");
    assert_ne!(addr.port(), 0, "port should be OS-assigned non-zero");

    // Skill client should be connected.
    let client = runtime
        .skill_client()
        .expect("skill client should be connected");
    let _ = Arc::strong_count(client); // verify the Arc is usable

    // Registry access works.
    let _registry = runtime.registry();
    let _pm = runtime.process_manager();

    // Shutdown is idempotent and clean.
    let results = runtime.shutdown().await;
    assert!(results.is_empty(), "no agents were spawned");
}

#[tokio::test]
async fn test_runtime_rejects_empty_model_config() {
    // An empty ModelConfig is invalid — the pool builder rejects it.
    let config = RuntimeConfig::headless().with_model_config(Default::default());
    let result = Runtime::new(config).await;
    assert!(
        result.is_err(),
        "empty model config should fail to initialize"
    );
}

#[tokio::test]
async fn test_runtime_registry_lifecycle() {
    // Verify the registry is accessible and supports the full CRUD cycle
    // through the Runtime portal.
    let runtime = Runtime::new(RuntimeConfig::headless())
        .await
        .expect("headless runtime should initialize");

    let registry = runtime.registry();
    assert!(registry.list().is_empty(), "registry starts empty");

    let blueprint = Arc::new(AgentBlueprint::new(
        "kind-a",
        "Kind A",
        agentik_skill::SkillTree::default(),
        agentik_core::tools::ToolProviderRegistry::new(),
    ));
    registry.register(blueprint);

    assert_eq!(registry.list(), vec!["kind-a".to_string()]);
    assert!(registry.get("kind-a").is_some());

    registry.unregister("kind-a");
    assert!(registry.list().is_empty());

    runtime.shutdown().await;
}

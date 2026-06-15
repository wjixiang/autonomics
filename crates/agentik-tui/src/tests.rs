//! Unit tests for the Agent Status Panel.

use super::state::AgentPanelState;
use super::tools::tool_user_facing_name;

fn fresh_panel() -> AgentPanelState {
    AgentPanelState::default()
}

#[test]
fn add_agent_creates_entry() {
    let mut p = fresh_panel();
    let id = uuid::Uuid::nil();
    p.add_agent(id, "test".to_string());
    assert_eq!(p.agents.len(), 1);
    assert_eq!(p.agents[0].title, "test");
}

#[test]
fn add_agent_deduplicates() {
    let mut p = fresh_panel();
    let id = uuid::Uuid::nil();
    p.add_agent(id, "test".to_string());
    p.add_agent(id, "test2".to_string());
    assert_eq!(p.agents.len(), 1);
    // Title should update to the better one.
    assert_eq!(p.agents[0].title, "test2");
}

#[test]
fn move_selection_clamps() {
    let mut p = fresh_panel();
    let id1 = uuid::Uuid::new_v4();
    let id2 = uuid::Uuid::new_v4();
    p.add_agent(id1, "A".into());
    p.add_agent(id2, "B".into());
    p.move_selection(5);
    assert_eq!(p.selected, 1);
    p.move_selection(-10);
    assert_eq!(p.selected, 0);
}

#[test]
fn toggle_selected_flips() {
    let mut p = fresh_panel();
    p.add_agent(uuid::Uuid::nil(), "A".into());
    assert!(!p.agents[0].expanded);
    p.toggle_selected();
    assert!(p.agents[0].expanded);
    p.toggle_selected();
    assert!(!p.agents[0].expanded);
}

#[test]
fn tool_user_facing_name_local() {
    let s = tool_user_facing_name("kms_local", &serde_json::json!({"path": "src/lib.rs"}));
    assert_eq!(s, "Inspect src/lib.rs");
}

#[test]
fn tool_user_facing_name_unknown_falls_back() {
    let s = tool_user_facing_name("kms_unknown", &serde_json::json!({"foo": "bar"}));
    assert!(s.starts_with("kms_unknown"));
}

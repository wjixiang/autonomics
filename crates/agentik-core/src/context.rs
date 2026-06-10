use std::collections::HashMap;

use async_trait::async_trait;

/// A versioned snapshot of external context data.
///
/// Implementors populate `data` with arbitrary key-value pairs.
/// The framework does not interpret any keys — it only checks
/// the version to decide whether to inject into memory.
#[derive(Debug, Clone)]
pub struct ContextSnapshot {
    pub data: HashMap<String, serde_json::Value>,
    pub version: u64,
}

impl Default for ContextSnapshot {
    fn default() -> Self {
        Self {
            data: HashMap::new(),
            version: 0,
        }
    }
}

/// Changes to apply to the context store.
#[derive(Debug, Clone, Default)]
pub struct ContextChanges {
    pub data: HashMap<String, serde_json::Value>,
}

/// Reactive context store interface.
///
/// Implementors maintain internal state and expose it via `read()`.
/// External mutations (tool results, file changes, etc.) are applied
/// via `write()`. The framework polls `read()` at each loop boundary
/// and injects a message into memory when the version changes.
#[async_trait]
pub trait AgentContext: Send + Sync {
    /// Return a clone of the current context state.
    async fn read(&self) -> ContextSnapshot;

    /// Apply external changes. Implementors should update internal state,
    /// bump the version, and persist if needed.
    async fn write(&self, changes: ContextChanges) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// Built-in in-memory implementation for tests and simple use cases
// ---------------------------------------------------------------------------

use tokio::sync::RwLock;

/// A simple in-memory context store.
///
/// Thread-safe via `RwLock`. Suitable for tests and agents that do not
/// need persistent context.
pub struct InMemoryAgentContext {
    inner: RwLock<ContextSnapshot>,
}

impl InMemoryAgentContext {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(ContextSnapshot::default()),
        }
    }
}

impl Default for InMemoryAgentContext {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentContext for InMemoryAgentContext {
    async fn read(&self) -> ContextSnapshot {
        self.inner.read().await.clone()
    }

    async fn write(&self, changes: ContextChanges) -> Result<(), String> {
        let mut guard = self.inner.write().await;
        guard.data.extend(changes.data);
        guard.version += 1;
        Ok(())
    }
}

/// Serialize a `ContextSnapshot` into a human-readable string for LLM injection.
///
/// Format:
/// ```text
/// [context-update v{version}]
/// key1: value1
/// key2: value2
/// ```
pub fn serialize_snapshot(snapshot: &ContextSnapshot) -> String {
    use serde_json::Value;

    let mut lines = vec![format!("[context-update v{}]", snapshot.version)];

    // Sort keys for deterministic output
    let mut keys: Vec<&String> = snapshot.data.keys().collect();
    keys.sort();

    for key in keys {
        let value = &snapshot.data[key];
        let formatted = match value {
            Value::String(s) => s.clone(),
            Value::Null => "null".to_string(),
            other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
        };
        lines.push(format!("{}: {}", key, formatted));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_in_memory_read_write() {
        let ctx = InMemoryAgentContext::new();

        // Initial read returns version 0, empty data
        let snap = ctx.read().await;
        assert_eq!(snap.version, 0);
        assert!(snap.data.is_empty());

        // Write some data
        let mut data = HashMap::new();
        data.insert("location".to_string(), json!("Beijing"));
        data.insert("diagnostics".to_string(), json!([]));
        ctx.write(ContextChanges { data }).await.unwrap();

        // Read again — version bumped, data present
        let snap = ctx.read().await;
        assert_eq!(snap.version, 1);
        assert_eq!(snap.data.get("location").unwrap(), &json!("Beijing"));
        assert_eq!(snap.data.get("diagnostics").unwrap(), &json!([]));
    }

    #[tokio::test]
    async fn test_in_memory_write_accumulates() {
        let ctx = InMemoryAgentContext::new();

        let mut data = HashMap::new();
        data.insert("a".to_string(), json!(1));
        ctx.write(ContextChanges { data }).await.unwrap();

        let mut data2 = HashMap::new();
        data2.insert("b".to_string(), json!(2));
        ctx.write(ContextChanges { data: data2 }).await.unwrap();

        let snap = ctx.read().await;
        assert_eq!(snap.version, 2);
        assert_eq!(snap.data.get("a").unwrap(), &json!(1));
        assert_eq!(snap.data.get("b").unwrap(), &json!(2));
    }

    #[test]
    fn test_serialize_snapshot_empty() {
        let snap = ContextSnapshot::default();
        let output = serialize_snapshot(&snap);
        assert_eq!(output, "[context-update v0]");
    }

    #[test]
    fn test_serialize_snapshot_with_data() {
        let mut data = HashMap::new();
        data.insert("location".to_string(), json!("Shanghai"));
        data.insert("errors".to_string(), json!(["type mismatch"]));
        let snap = ContextSnapshot { data, version: 3 };
        let output = serialize_snapshot(&snap);

        assert!(output.starts_with("[context-update v3]"));
        // Keys are sorted, so "errors" comes before "location"
        assert!(output.contains("errors: [\"type mismatch\"]"));
        assert!(output.contains("location: Shanghai"));
    }

    #[test]
    fn test_serialize_snapshot_primitive_types() {
        let mut data = HashMap::new();
        data.insert("count".to_string(), json!(42));
        data.insert("active".to_string(), json!(true));
        data.insert("note".to_string(), json!(null));
        data.insert("name".to_string(), json!("test"));
        let snap = ContextSnapshot { data, version: 1 };
        let output = serialize_snapshot(&snap);

        assert!(output.contains("active: true"));
        assert!(output.contains("count: 42"));
        assert!(output.contains("name: test"));
        assert!(output.contains("note: null"));
    }
}

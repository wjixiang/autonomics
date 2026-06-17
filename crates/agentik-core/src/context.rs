use async_trait::async_trait;

/// Optional callback for dynamic context injection.
///
/// If provided, the agent calls `poll()` at each loop boundary (before
/// each LLM request). When it returns `Some(text)`, the text is injected
/// as a user message into memory. Return `None` to skip.
///
/// This replaces the previous `AgentContext` reactive store with a much
/// simpler callback interface.
#[async_trait]
pub trait ContextProvider: Send + Sync {
    async fn poll(&self) -> Option<String>;
}

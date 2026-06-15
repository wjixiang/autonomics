//! # Design Principles
//!
//! Complete separation of tool invocation from tool execution within the agent system.
//! The core agent loop is behaviorally uniform across all agents — hardcoded logic provides
//! only generic capabilities (request–response cycling, lifecycle management, effect application)
//! and never encodes agent-specific behavior, tool selection, or prompt engineering at the
//! structural level. Agent personality and tooling are configured exclusively through the
//! toolset and system prompt, not through code paths.

use std::{sync::Arc, time::Duration, time::UNIX_EPOCH};

use crate::context::{AgentContext, serialize_snapshot};
use crate::message_ext::AgentMessageExt;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::types::messages::{ContentBlock, Message, Role};
use agentik_sdk::types::tools::ToolUse;
use futures::StreamExt;
use tracing::{Level, event, span};
use uuid::Uuid;

use agentik_sdk::types::ToolCallResponseContent;
use agentik_sdk::types::{AgentEvent, ToolCallResponse};

use crate::prompt::system_prompt_builder;
use crate::tools::ToolEffect;

use crate::{
    error::{AgentError, Retryable},
    lifecycle::AgentLifecycle,
    memory::Memory,
    storage::{AgentSnapshot, AgentSnapshotStorage},
    tools::{ToolRegistration, Toolset},
};

#[derive(Clone)]
pub struct AgentConfig {
    pub max_iterations: usize,
    pub max_retries: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 1000,
            max_retries: 10,
        }
    }
}

pub struct Agent {
    pub(crate) id: Uuid,
    pub(crate) model_pool: Arc<ModelPool>,
    pub(crate) memory: Memory,
    pub(crate) lifecycle: AgentLifecycle,
    pub(crate) toolset: Toolset,
    pub(crate) config: AgentConfig,
    pub(crate) storage: Option<Arc<dyn AgentSnapshotStorage>>,
    pub(crate) token_budget: TokenBudget,
    pub(crate) ctx: Arc<dyn AgentContext>,
    pub(crate) last_context_version: u64,
    pub(crate) system_prompt_section: Option<String>,
    pub(crate) system_prompt_identity: Option<String>,
    /// Optional event channel for streaming progress to external observers.
    pub event_tx: Option<tokio::sync::mpsc::UnboundedSender<agentik_sdk::types::AgentUiEvent>>,
    /// Currently selected model name. If None, falls back to round-robin.
    pub(crate) current_model_name: Option<String>,
}

impl Agent {
    pub fn builder() -> crate::agent_builder::AgentBuilder {
        crate::agent_builder::AgentBuilder::new()
    }

    /// Send an event to the optional observation channel.
    fn send_event(&self, event: agentik_sdk::types::AgentUiEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }
    }

    /// Switch to a different model by name. No-op if the name is not in the pool.
    pub fn select_model(&mut self, name: &str) {
        if self.model_pool.get_model_by_name(name).is_ok() {
            self.current_model_name = Some(name.to_string());
        }
    }

    /// Returns the currently selected model name, or None for round-robin.
    pub fn current_model(&self) -> Option<&str> {
        self.current_model_name.as_deref()
    }

    /// Returns the agent's unique ID.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Register a single tool.
    pub fn register_tool(&mut self, registration: ToolRegistration) -> Result<(), AgentError> {
        self.toolset
            .register(registration)
            .map_err(AgentError::Tool)?;
        Ok(())
    }

    /// Register multiple tools at once.
    pub fn register_tools(
        &mut self,
        registrations: Vec<ToolRegistration>,
    ) -> Result<(), AgentError> {
        self.toolset
            .register_all(registrations)
            .map_err(AgentError::Tool)?;
        Ok(())
    }

    pub async fn snapshot(&self) -> AgentSnapshot {
        let snapshot = AgentSnapshot {
            ts: std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            agent_id: self.id,
            agent_status: *self.lifecycle.status(),
            memory: self.memory.clone(),
        };

        if let Some(storage) = self.storage.clone() {
            let _ = storage.as_ref().create_snapshot(snapshot.clone()).await;
        }

        snapshot
    }

    pub fn inject_message(&mut self, user_content: Vec<ContentBlock>) -> Result<(), AgentError> {
        let message = Message {
            id: Uuid::new_v4().to_string(),
            type_: "message".to_string(),
            role: Role::User,
            content: user_content,
            model: None,
            stop_reason: None,
            stop_sequence: None,
            usage: None,
            request_id: None,
        };
        self.memory.remember(message)?;
        Ok(())
    }

    pub async fn start(&mut self) -> Result<(), AgentError> {
        self.lifecycle.set_running();
        self.send_event(agentik_sdk::types::AgentUiEvent::LlmResponse(
            "🤖 Agent started".to_string(),
        ));

        // Initial context injection
        self.inject_context_if_changed().await;

        let mut iteration = 0;
        let mut consecutive_retries = 0;
        let mut retry_feedback: Option<String> = None;

        while self.lifecycle.is_running() && iteration < self.config.max_iterations {
            iteration += 1;
            match self.agent_workflow(retry_feedback.take()).await {
                Ok(()) => consecutive_retries = 0,
                Err(e) if e.is_retryable() && consecutive_retries < self.config.max_retries => {
                    consecutive_retries += 1;
                    tracing::warn!(
                        "retryable error at iteration {}/{}, retry {}/{}: {e}",
                        iteration,
                        self.config.max_iterations,
                        consecutive_retries,
                        self.config.max_retries
                    );
                    let delay = Duration::from_secs(1) * (1 << (consecutive_retries - 1));
                    tracing::warn!("exponential backoff: sleeping {delay:?} before retry");
                    tokio::time::sleep(delay).await;

                    // Record error feedback
                    self.memory.remember(Message::user(e.retry_message()))?;

                    continue;
                }
                Err(e) => {
                    tracing::error!("{}", e.to_string());
                    self.send_event(agentik_sdk::types::AgentUiEvent::Error(format!("{}", e)));
                    return Err(AgentError::WorkflowFailed {
                        iteration,
                        error: Box::new(e),
                    });
                }
            }
        }

        if iteration >= self.config.max_iterations {
            return Err(AgentError::MaxIterations(self.config.max_iterations));
        }

        self.send_event(agentik_sdk::types::AgentUiEvent::Done);
        Ok(())
    }

    /// Core agent workflow
    ///
    /// Basic process: build context -> request API -> execute tool calls -> append to memory
    async fn agent_workflow(&mut self, retry_feedback: Option<String>) -> Result<(), AgentError> {
        if let Some(feedback) = retry_feedback {
            self.inject_message(vec![ContentBlock::Text { text: feedback }])
                .unwrap();
        }

        // Context injection at loop boundary (before building context for LLM)
        self.inject_context_if_changed().await;

        let context = self.build_context().await?;
        self.send_event(agentik_sdk::types::AgentUiEvent::Requesting);
        let response_message = self.request(context).await?;
        event!(Level::INFO, "",);
        let last_usage = response_message.usage.clone().unwrap_or_default();

        // Emit LLM text and thinking content for UI observation
        for block in &response_message.content {
            match block {
                ContentBlock::Thinking { thinking, .. } if !thinking.is_empty() => {
                    self.send_event(agentik_sdk::types::AgentUiEvent::Thinking(thinking.clone()));
                }
                ContentBlock::Text { text } if !text.is_empty() => {
                    self.send_event(agentik_sdk::types::AgentUiEvent::LlmResponse(text.clone()));
                }
                _ => {}
            }
        }

        self.token_budget.latest_usage = last_usage.input_tokens + last_usage.output_tokens;

        // Always remember the LLM response so the final text (if any) is preserved
        // in conversation history before we decide whether to terminate.
        self.memory.remember(response_message.clone())?;

        let toolcalls = self.extract_toolcalls(&response_message);

        // No tool calls in the response means the agent has finished its work.
        // This aligns with the LLM's trained prior ("produce final text = done"),
        // removing the retry-loop failure mode where the model never explicitly
        // calls `attempt_complete`. The lifecycle is flipped to IDLE so the
        // outer `start()` loop exits.
        if toolcalls.is_empty() {
            self.lifecycle.set_idle();
            return Ok(());
        }

        for tc in &toolcalls {
            self.send_event(agentik_sdk::types::AgentUiEvent::ToolCall {
                name: tc.name.clone(),
                input: tc.input.clone(),
            });
        }

        let tool_results = self.toolset.execute(&toolcalls).await?;
        tracing::debug!(?tool_results, "tool execution results");

        for tr in &tool_results {
            let result_text: String = tr
                .content
                .iter()
                .filter_map(|c| match c {
                    ToolCallResponseContent::Text(t) => Some(t.as_str()),
                    ToolCallResponseContent::Image(_) => None,
                })
                .collect::<Vec<_>>()
                .join("");
            self.send_event(agentik_sdk::types::AgentUiEvent::ToolResult {
                ok: !tr.is_error.unwrap_or_default(),
                content: result_text,
            });
        }

        for tr in &tool_results {
            let text: String = tr
                .content
                .iter()
                .filter_map(|c| match c {
                    ToolCallResponseContent::Text(t) => Some(t.as_str()),
                    ToolCallResponseContent::Image(_) => None,
                })
                .collect::<Vec<_>>()
                .join("");
            self.memory.remember(Message::tool_result(
                tr.tool_use_id.clone(),
                text,
                tr.is_error.unwrap_or_default(),
            ))?;
        }

        self.handle_effect(&tool_results).await;

        // Context injection after tool execution (captures any writes that happened
        // during tool execution, e.g. state changes triggered by mutation tools)
        self.inject_context_if_changed().await;

        Ok(())
    }

    /// Check the context store for a version change. If detected, serialize
    /// the snapshot into a User message and append to memory.
    async fn inject_context_if_changed(&mut self) {
        let snapshot = self.ctx.read().await;
        if snapshot.version > self.last_context_version {
            self.last_context_version = snapshot.version;
            if !snapshot.data.is_empty() {
                let msg = serialize_snapshot(&snapshot);
                let _ = self.memory.remember(Message::user(msg));
            }
        }
    }

    /// Apply agent-level effects declared by tool results (e.g. lifecycle transitions).
    async fn handle_effect(&mut self, tool_results: &[ToolCallResponse]) {
        let effects: Vec<ToolEffect> = tool_results
            .iter()
            .flat_map(|ts| ts.effects.clone())
            .collect();

        effects.iter().for_each(|e| match e {
            ToolEffect::AttemptComplete => {
                self.lifecycle.set_idle();
            }
            ToolEffect::Abort => {
                self.lifecycle.set_aborted();
            }
        });
    }

    async fn build_context(&mut self) -> Result<Vec<Message>, AgentError> {
        use crate::prompt::context::Context;

        let mut builder =
            system_prompt_builder::SystemPromptBuilder::default().build_tooluse_guidance();

        if let Some(ref identity) = self.system_prompt_identity {
            builder = builder.with_identity(identity);
        }

        if let Some(ref extra) = self.system_prompt_section {
            builder = builder.with_extra_section(extra);
        }

        let system_prompt = builder.parse();

        let context_messages = self.memory.render_context()?.to_vec();

        let context = Context::new()
            .with_system_prompt(system_prompt)
            .with_conversations(context_messages)
            .build();

        Ok(context)
    }

    async fn request(&mut self, context: Vec<Message>) -> Result<Message, AgentError> {
        let span = span!(Level::TRACE, "API Request");
        let _enter = span.enter();

        let model = if let Some(name) = &self.current_model_name {
            self.model_pool
                .get_model_by_name(name)
                .unwrap_or_else(|_| self.model_pool.get_model_roundrobin().unwrap())
        } else {
            self.model_pool.get_model_roundrobin().unwrap()
        };

        let est_total_token = self.token_budget.estimate_total_token(context.len() as u64);

        if est_total_token * 9 > (model.model_info.context_length * 10) {
            tracing::debug!(
                est_tokens = est_total_token,
                context_length = model.model_info.context_length,
                "context pressure detected, compacting"
            );
            self.memory.compact(model.as_ref()).await?;
        }

        let mut stream = model
            .request_stream(context, self.toolset.tools().as_ref())
            .await?;

        while let Some(event) = stream.next().await {
            let stream_event = match event {
                Ok(e) => e,
                Err(e) => {
                    // poll_next already skips lagged events, but handle
                    // them defensively here as well (belt-and-suspenders).
                    match &e {
                        agentik_sdk::types::AnthropicError::StreamError(msg)
                            if msg.starts_with("Stream lagged:") =>
                        {
                            tracing::debug!("skipping lagged event: {e}");
                            continue;
                        }
                        _ => {
                            tracing::warn!("stream event error: {e}; breaking stream loop");
                            break;
                        }
                    }
                }
            };

            if let Some(agent_event) = AgentEvent::from_stream_event(&stream_event) {
                self.send_event(agent_event);
            }
        }
        // NB: do NOT emit `AgentEvent::Done` here. `Done` is a
        // lifecycle signal that the TUI uses to flip its `agent_running`
        // flag and re-enable the input field. Emitting it after every
        // LLM response — including the intermediate ones that are
        // followed by tool calls and another round-trip — caused the
        // TUI to think the agent had finished mid-iteration, which
        // collapsed the spinner into the "Enter to type" hint while
        // tool calls and the next streaming response were still in
        // flight. The lifecycle-based `Done` at the bottom of
        // `start()` is the single correct emission point.
        // (See also `AgentEvent::from_stream_event` for
        // `MessageStop`, which returns `None` for the same reason.)

        let response = stream.final_message().await?;

        tracing::debug!(?response, "LLM response");

        Ok(response)
    }

    fn extract_toolcalls(&self, message: &Message) -> Vec<ToolUse> {
        message
            .content
            .iter()
            .filter_map(|c| {
                if let ContentBlock::ToolUse { id, name, input } = c {
                    Some(ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

#[derive(Default)]
pub struct TokenBudget {
    append_tokens: u64,
    latest_usage: u64,
}
impl TokenBudget {
    pub fn count_token_est(&self, msg: &Message) -> u64 {
        if let Some(usage) = &msg.usage {
            return usage.input_tokens;
        }

        let content_str = serde_json::to_string(&msg.content)
            .expect("Convert message to JSON string failed during counting token budget");

        content_str.len() as u64 / 4
    }

    pub fn increment_new_msg(&mut self, msg: &Message) {
        self.append_tokens = self.count_token_est(msg);
    }

    pub fn estimate_total_token(&self, system_prompt_token: u64) -> u64 {
        self.append_tokens + self.latest_usage + system_prompt_token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{AgentContext, ContextChanges, ContextSnapshot};
    use crate::testing::dummy_model_info;
    use agentik_sdk::model::Model;
    use agentik_sdk::model::model_info::ModelInfo;
    use agentik_sdk::provider::client::MockApiClient;
    use agentik_sdk::types::AgentEvent;
    use agentik_sdk::types::messages::{ContentBlock, Message, Role};
    use agentik_sdk::types::shared::Usage;

    // ── Mock AgentContext ──────────────────────────────────────

    struct MockCtx;

    #[async_trait::async_trait]
    impl AgentContext for MockCtx {
        async fn read(&self) -> ContextSnapshot {
            ContextSnapshot::default()
        }

        async fn write(&self, _changes: ContextChanges) -> Result<(), String> {
            Ok(())
        }
    }

    // ── Helpers ────────────────────────────────────────────────

    fn test_model_info() -> ModelInfo {
        dummy_model_info("test-model")
    }

    #[allow(dead_code)]
    fn test_final_message(text: &str) -> Message {
        Message {
            id: "msg_test".into(),
            type_: "message".into(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
            model: Some("test-model".into()),
            stop_reason: None,
            stop_sequence: None,
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 20,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
                server_tool_use: None,
                service_tier: None,
            }),
            request_id: None,
        }
    }

    /// Build a minimal agent with an event receiver wired up.
    async fn build_test_agent(
        mock_api: MockApiClient,
    ) -> (Agent, tokio::sync::mpsc::UnboundedReceiver<AgentEvent>) {
        let mut model_pool = ModelPool::new();
        model_pool.add_model(Model::new(test_model_info(), mock_api));

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let mut agent = Agent::builder()
            .with_model_pool(Arc::new(model_pool))
            .with_context(Arc::new(MockCtx))
            .with_config(AgentConfig {
                max_iterations: 5,
                max_retries: 0,
            })
            .build()
            .await
            .unwrap();

        agent.event_tx = Some(tx);
        (agent, rx)
    }

    /// Collect all events from the receiver until channel closes or timeout.
    async fn collect_events(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    ) -> Vec<AgentEvent> {
        let mut events = vec![];
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        events
    }

    // ── Tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_events_received_on_simple_text_response() {
        // TODO: configure MockApiClient.expect_request_stream() to return
        // MessageStream::from_events(vec![...], test_final_message("hello"))
        //
        let mock = MockApiClient::new();
        // mock.expect_request_stream()
        //     .returning(|_, _, _| { /* return mock stream */ });

        let (mut agent, mut rx) = build_test_agent(mock).await;

        tokio::spawn(async move {
            let _ = agent.start().await;
        });

        let _events = collect_events(&mut rx).await;

        // Verify event sequence contains:
        // LlmResponse("🤖 Agent started") → Requesting → TextDelta → ... → LlmResponse("hello") → Done
    }
}

//! # Design Principles
//!
//! Complete separation of tool invocation from tool execution within the agent system.
//! The core agent loop is behaviorally uniform across all agents — hardcoded logic provides
//! only generic capabilities (request–response cycling, lifecycle management, effect application)
//! and never encodes agent-specific behavior, tool selection, or prompt engineering at the
//! structural level. Agent personality and tooling are configured exclusively through the
//! toolset and system prompt, not through code paths.

use std::{sync::Arc, time::Duration, time::UNIX_EPOCH};

use crate::context::ContextProvider;
use crate::message_ext::AgentMessageExt;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::types::messages::{ContentBlock, Message, Role};
use agentik_sdk::types::tools::{ToolResultContent, ToolUse};
use futures::StreamExt;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event, span};
use uuid::Uuid;

use agentik_sdk::types::AgentEvent;

use crate::prompt::system_prompt_builder;
use crate::tools::ToolEffect;

use crate::{
    error::{AgentError, Retryable},
    lifecycle::AgentLifecycle,
    memory::Memory,
    skill::SharedSkillRuntime,
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
    pub(crate) context_provider: Option<Arc<dyn ContextProvider>>,
    pub(crate) system_prompt_section: Option<String>,
    pub(crate) system_prompt_identity: Option<String>,
    /// Optional active skill workflow. When set, the agent is constrained
    /// to the current step's `allowed_tools` each turn and the step's todo
    /// progress is injected into the system prompt.
    pub(crate) skill_runtime: Option<SharedSkillRuntime>,
    /// Optional event channel for streaming progress to external observers.
    pub event_tx: Option<tokio::sync::mpsc::UnboundedSender<agentik_sdk::types::AgentEvent>>,
    /// Currently selected model name. If None, falls back to round-robin.
    pub(crate) current_model_name: Option<String>,
    /// External cancellation signal, Cloned out to callers so they can
    /// interrupt the agent loop cooperatively.
    pub(crate) cancel_token: CancellationToken,
}

impl Agent {
    pub fn builder() -> crate::agent_builder::AgentBuilder {
        crate::agent_builder::AgentBuilder::new()
    }

    /// Send an event to the optional observation channel.
    fn send_event(&self, event: agentik_sdk::types::AgentEvent) {
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

    /// Wire an event channel for external observation (e.g. TUI, tests).
    pub fn set_event_tx(
        &mut self,
        tx: tokio::sync::mpsc::UnboundedSender<agentik_sdk::types::AgentEvent>,
    ) {
        self.event_tx = Some(tx);
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

    /// Override the system prompt identity line.
    pub fn set_system_prompt_identity(&mut self, identity: impl Into<String>) {
        self.system_prompt_identity = Some(identity.into());
    }

    /// Override the system prompt section.
    pub fn set_system_prompt_section(&mut self, section: impl Into<String>) {
        self.system_prompt_section = Some(section.into());
    }

    pub async fn snapshot(&self) -> AgentSnapshot {
        let snapshot = AgentSnapshot {
            snapshot_id: Uuid::new_v4(),
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

    pub fn lifecycle_status(&self) -> agentik_types::AgentLifecycleStatus {
        *self.lifecycle.status()
    }

    pub fn is_running(&self) -> bool {
        self.lifecycle.is_running()
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

    /// Replace the cancellation token before the next [`start`] call.
    ///
    /// Because `CancellationToken` is one-shot (once cancelled it stays
    /// cancelled), callers that reuse an `Agent` across multiple runs must
    /// inject a fresh token each time — otherwise a prior cancel would
    /// prevent every subsequent `start()` from entering its loop.
    pub fn set_cancel_token(&mut self, token: CancellationToken) {
        self.cancel_token = token;
    }

    pub async fn run(&mut self) {}

    pub async fn start(&mut self) -> Result<(), AgentError> {
        self.lifecycle.set_running();
        self.send_event(agentik_sdk::types::AgentEvent::LlmResponse(
            "🤖 Agent started".to_string(),
        ));

        let mut iteration = 0;
        let mut consecutive_retries = 0;
        let mut retry_feedback: Option<String> = None;
        let cancelled = self.cancel_token.clone();

        'outer: while self.lifecycle.is_running()
            && iteration < self.config.max_iterations
            && !self.cancel_token.is_cancelled()
        {
            iteration += 1;
            // Cooperatively race the workflow against the external cancel signal.
            // `select!` awaits both branches simultaneously; whichever resolves
            // first wins. `cancelled.cancelled()` returns a Future that resolves
            // when `cancel_token.cancel()` is called from outside (e.g. user presses Esc).
            tokio::select! {
                result = self.agent_workflow(retry_feedback.take()) => {
                    match result {
                        Ok(()) => {
                            consecutive_retries = 0;
                            self.snapshot().await;

                            // Poll background tasks: if any completed, inject
                            // the real result into memory and loop back to
                            // re-enter agent_workflow (which will call the LLM
                            // again with the real result).
                            while self.toolset.has_background_tasks().await {
                                let completed = self
                                    .toolset
                                    .poll_completed_tasks()
                                    .await;
                                if !completed.is_empty() {
                                    for (id, ok, content) in &completed {
                                        self.send_event(
                                            agentik_sdk::types::AgentEvent::ToolBackgroundComplete {
                                                id: id.clone(),
                                                ok: *ok,
                                                content: content.clone(),
                                            },
                                        );
                                        self.memory.remember(Message::tool_result(
                                            id.clone(),
                                            content.clone(),
                                            !ok,
                                        ))?;
                                    }
                                    // Re-enter agent_workflow with the new results.
                                    continue 'outer;
                                }
                                // No completions yet — brief sleep, race against cancel.
                                tokio::select! {
                                    _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                                    _ = cancelled.cancelled() => {
                                        self.lifecycle.set_aborted();
                                        self.snapshot().await;
                                        self.send_event(AgentEvent::Error("Task cancelled by user".into()));
                                        return Err(AgentError::Cancelled);
                                    }
                                }
                            }
                        }
                        Err(AgentError::CompactionRebuild) => {
                            tracing::info!("compaction rebuild, re-entering workflow");
                            continue;
                        }
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
                            self.send_event(agentik_sdk::types::AgentEvent::Error(format!("{}", e)));
                            self.snapshot().await;
                            return Err(AgentError::WorkflowFailed {
                                iteration,
                                error: Box::new(e),
                            });
                        }
                    }
                }
                _ = cancelled.cancelled() => {
                    self.lifecycle.set_aborted();
                    self.snapshot().await;
                    self.send_event(AgentEvent::Error("Task cancelled by user".into()));
                    return Err(AgentError::Cancelled);
                }
            }
        }

        if iteration >= self.config.max_iterations {
            // Emit a terminal event so any observing client (SSE/UI) is never
            // left hanging when the iteration budget is exhausted — every
            // return path from `start()` must emit Done or Error.
            self.send_event(agentik_sdk::types::AgentEvent::Error(format!(
                "max iterations ({}) reached",
                self.config.max_iterations
            )));
            self.snapshot().await;
            return Err(AgentError::MaxIterations(self.config.max_iterations));
        }

        self.snapshot().await;
        self.send_event(agentik_sdk::types::AgentEvent::Done);
        Ok(())
    }

    /// Core agent workflow
    ///
    /// Basic process: build context -> request API -> execute tool calls -> append to memory.
    /// Returns `Err(AgentError::CompactionRebuild)` when a compaction occurred and the
    /// caller should re-enter this method with fresh context.
    async fn agent_workflow(&mut self, retry_feedback: Option<String>) -> Result<(), AgentError> {
        if let Some(feedback) = retry_feedback {
            self.inject_message(vec![ContentBlock::Text { text: feedback }])
                .unwrap();
        }

        // Poll context provider for dynamic injection
        self.poll_context_provider().await;

        let context = self.build_context().await?;

        // If a skill is active, restrict the LLM's toolset to the current
        // step's allowed tools for this turn. The same whitelist is enforced
        // again at execution time below.
        let allowed = self.current_allowed_tools().await;

        self.send_event(agentik_sdk::types::AgentEvent::Requesting);
        let response_message = self.request(context, allowed.as_deref()).await?;
        event!(Level::INFO, "",);
        let last_usage = response_message.usage.clone().unwrap_or_default();

        // Emit LLM text and thinking content for UI observation
        for block in &response_message.content {
            match block {
                ContentBlock::Thinking { thinking, .. } if !thinking.is_empty() => {
                    self.send_event(agentik_sdk::types::AgentEvent::Thinking(thinking.clone()));
                }
                ContentBlock::Text { text } if !text.is_empty() => {
                    self.send_event(agentik_sdk::types::AgentEvent::LlmResponse(text.clone()));
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
            self.send_event(agentik_sdk::types::AgentEvent::ToolCall {
                name: tc.name.clone(),
                input: tc.input.clone(),
            });
        }

        let tool_results = self.toolset.execute(&toolcalls, allowed.as_deref()).await?;
        tracing::debug!(?tool_results, "tool execution results");

        for tr in &tool_results {
            let is_placeholder = matches!(&tr.content, ToolResultContent::Text(t) if t.contains("is running in backend"));
            if is_placeholder {
                // Find the original tool name from toolcalls by matching id.
                let name = toolcalls
                    .iter()
                    .find(|tc| tc.id == tr.tool_use_id)
                    .map(|tc| tc.name.as_str())
                    .unwrap_or("unknown");
                self.send_event(agentik_sdk::types::AgentEvent::ToolCallBackground {
                    id: tr.tool_use_id.clone(),
                    name: name.to_string(),
                });
            } else {
                self.send_event(agentik_sdk::types::AgentEvent::ToolResult {
                    ok: !tr.is_error.unwrap_or_default(),
                    content: tr.text_content(),
                });
            }
        }

        for tr in &tool_results {
            self.memory.remember(Message::tool_result(
                tr.tool_use_id.clone(),
                tr.text_content(),
                tr.is_error.unwrap_or_default(),
            ))?;
        }

        self.handle_effect(&tool_results).await;

        Ok(())
    }

    /// Poll the optional context provider for dynamic data.
    /// If it returns Some(text), inject as a user message.
    async fn poll_context_provider(&mut self) {
        if let Some(provider) = &self.context_provider {
            if let Some(text) = provider.poll().await {
                let _ = self.memory.remember(Message::user(text));
            }
        }
    }

    /// Return the tool whitelist for the active skill's current step, or
    /// `None` when no skill is attached (meaning all tools are allowed).
    async fn current_allowed_tools(&self) -> Option<Vec<String>> {
        let rt = self.skill_runtime.as_ref()?;
        let guard = rt.lock().await;
        Some(guard.allowed_tools_for_current_step())
    }

    /// Apply agent-level effects declared by tool results (e.g. lifecycle transitions).
    async fn handle_effect(&mut self, tool_results: &[agentik_sdk::types::tools::ToolResult]) {
        let effects: Vec<ToolEffect> = tool_results
            .iter()
            .flat_map(|ts| ts.effects.clone())
            .collect();

        effects.iter().for_each(|e| match e {
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

        // Inject the active skill's current step / todo progress.
        if let Some(rt) = &self.skill_runtime {
            let section = rt.lock().await.current_prompt_section();
            if !section.is_empty() {
                builder = builder.with_extra_section(section);
            }
        }

        let system_prompt = builder.parse();

        let context_messages = self.memory.render_context()?.to_vec();

        let context = Context::new()
            .with_system_prompt(system_prompt)
            .with_conversations(context_messages)
            .build();

        Ok(context)
    }

    async fn request(
        &mut self,
        context: Vec<Message>,
        allowed: Option<&[String]>,
    ) -> Result<Message, AgentError> {
        let span = span!(Level::TRACE, "API Request");
        let _enter = span.enter();

        let model = if let Some(name) = &self.current_model_name {
            self.model_pool
                .get_model_by_name(name)
                .unwrap_or_else(|_| self.model_pool.get_model_roundrobin().unwrap())
        } else {
            self.model_pool.get_model_roundrobin().unwrap()
        };

        // Accurate overflow detection using full message-list token estimation,
        // matching OpenCode's `compactIfNeeded()` logic.
        let conversation_msgs = self.memory.render_context()?;
        if self.token_budget.should_compact(
            &conversation_msgs,
            model.model_info.context_length,
            model.model_info.max_output_tokens,
        ) {
            tracing::debug!(
                context_length = model.model_info.context_length,
                max_output_tokens = model.model_info.max_output_tokens,
                "context pressure detected, compacting"
            );
            self.memory.compact(model.as_ref()).await?;
            // Rebuild context after compaction
            return Err(AgentError::CompactionRebuild);
        }

        let all_tools = self.toolset.tools_filtered(allowed);

        let mut stream = model.request_stream(context, &all_tools).await?;

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
        tracing::info!("stream loop exited, awaiting final_message");
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

        let response =
            tokio::time::timeout(std::time::Duration::from_secs(5), stream.final_message())
                .await
                .map_err(|e| AgentError::WorkflowFailed {
                    iteration: 0,
                    error: Box::new(AgentError::MissingConfig(format!(
                        "final_message() timed out: {e}"
                    ))),
                })??;

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

/// Default token buffer before compaction triggers (matching OpenCode).
const COMPACTION_BUFFER_TOKENS: u64 = 20_000;
/// Default number of tokens to preserve in the "recent" tail during compaction.
const DEFAULT_KEEP_TOKENS: u64 = 8_000;

#[derive(Default)]
pub struct TokenBudget {
    append_tokens: u64,
    latest_usage: u64,
}
impl TokenBudget {
    /// Estimate token count for a single message.
    ///
    /// Uses the actual `input_tokens` from the API response if available,
    /// otherwise falls back to the chars/4 heuristic (matching OpenCode's
    /// `Token.estimate()`).
    pub fn count_token_est(&self, msg: &Message) -> u64 {
        if let Some(usage) = &msg.usage {
            return usage.input_tokens;
        }

        let content_str = serde_json::to_string(&msg.content)
            .expect("Convert message to JSON string failed during counting token budget");

        content_str.len() as u64 / 4
    }

    /// Estimate token count for an entire message list.
    ///
    /// This is the accurate version used for compaction decisions.
    /// It sums per-message estimates (preferring API-reported tokens when
    /// available, falling back to chars/4).
    pub fn estimate_messages_tokens(&self, messages: &[Message]) -> u64 {
        messages.iter().map(|m| self.count_token_est(m)).sum()
    }

    pub fn increment_new_msg(&mut self, msg: &Message) {
        self.append_tokens = self.count_token_est(msg);
    }

    pub fn estimate_total_token(&self, system_prompt_token: u64) -> u64 {
        self.append_tokens + self.latest_usage + system_prompt_token
    }

    /// Determine whether the conversation should be compacted.
    ///
    /// Uses the accurate full-message-list token estimate rather than the
    /// crude append-only heuristic. Mirrors OpenCode's `compactIfNeeded()`:
    /// triggers when total tokens >= context - max(output_tokens, buffer).
    pub fn should_compact(
        &self,
        messages: &[Message],
        context_length: u64,
        max_output_tokens: u64,
    ) -> bool {
        let total = self.estimate_messages_tokens(messages);
        let reserve = max_output_tokens.max(COMPACTION_BUFFER_TOKENS);
        let usable = context_length.saturating_sub(reserve);
        total >= usable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::dummy_model_info;
    use agentik_sdk::model::Model;
    use agentik_sdk::model::model_info::ModelInfo;
    use agentik_sdk::provider::client::MockApiClient;
    use agentik_sdk::types::AgentEvent;
    use agentik_sdk::types::messages::{ContentBlock, Message, Role};
    use agentik_sdk::types::shared::Usage;

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
        model_pool.add_model(Model::with_client(test_model_info(), mock_api));

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let mut agent = Agent::builder()
            .with_model_pool(Arc::new(model_pool))
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

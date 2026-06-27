use futures::future::join_all;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::tools::task_runtime::TaskStatus;

use super::DynToolFunction;
use super::error::ToolError;
use super::task_runtime::TaskEntry;
use agentik_sdk::types::ToolDefinition;
use agentik_sdk::types::ToolEffect;
use agentik_sdk::types::tools::{ToolResult, ToolUse};

#[derive(Clone)]
pub struct ToolRegistration {
    pub definition: ToolDefinition,
    pub implementation: std::sync::Arc<dyn DynToolFunction>,
    pub effects: Vec<ToolEffect>,
}

impl ToolRegistration {
    pub fn new(
        definition: ToolDefinition,
        implementation: std::sync::Arc<dyn DynToolFunction>,
        effects: Vec<ToolEffect>,
    ) -> Self {
        Self {
            definition,
            implementation,
            effects,
        }
    }
}

impl<T: super::ToolFunction + 'static> From<T> for ToolRegistration {
    fn from(tool: T) -> Self {
        let definition = tool.definition();
        let effects = tool.effects();
        Self {
            definition,
            // T: ToolFunction implies T: DynToolFunction via the blanket impl,
            // so this coercion is automatic.
            implementation: std::sync::Arc::new(tool),
            effects,
        }
    }
}

pub struct Toolset {
    tools: HashMap<String, ToolRegistration>,
    tasks: Arc<Mutex<Vec<TaskEntry>>>,
}

impl Default for Toolset {
    fn default() -> Self {
        Self::new()
    }
}

impl Toolset {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn register(&mut self, registration: ToolRegistration) -> Result<(), ToolError> {
        let name = registration.definition.name.clone();
        if self.tools.contains_key(&name) {
            return Err(ToolError::RegistryError {
                message: format!("Tool '{}' is already registered", name),
            });
        }
        self.tools.insert(name, registration);
        Ok(())
    }

    pub fn register_all(&mut self, registrations: Vec<ToolRegistration>) -> Result<(), ToolError> {
        for reg in registrations {
            self.register(reg)?;
        }
        Ok(())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Return a clone of the shared task-list handle.
    ///
    /// Used by builtin tools (e.g. `view_task_results`) that need to
    /// inspect background tasks without going through the agent loop.
    pub fn tasks_handle(&self) -> Arc<Mutex<Vec<TaskEntry>>> {
        self.tasks.clone()
    }

    pub async fn execute(
        &self,
        toolcalls: &[ToolUse],
        allowed_tools: Option<&[String]>,
        notify_tx: Option<super::task_runtime::BgTaskNotifyTx>,
    ) -> Result<Vec<ToolResult>, ToolError> {
        let mut effects_map: HashMap<String, Vec<ToolEffect>> = HashMap::new();
        let mut immediate_results: Vec<ToolResult> = Vec::new();

        for tc in toolcalls {
            // 当 allowed_tools 存在时，跳过不在白名单内的工具
            if let Some(allowed) = allowed_tools
                && !allowed.contains(&tc.name)
            {
                immediate_results.push(ToolResult::error_with_id(
                    tc.id.clone(),
                    format!(
                        "tool '{}' is not available in current skill context",
                        tc.name
                    ),
                ));
                continue;
            }

            let Some(registration) = self.tools.get(&tc.name) else {
                continue;
            };
            let effects = registration.effects.clone();
            effects_map.insert(tc.id.clone(), effects);

            if let Err(e) = registration.implementation.validate_input(&tc.input) {
                let mut result = ToolResult::error_with_id(tc.id.clone(), e.to_string());
                result.effects = effects_map.remove(&tc.id).unwrap_or_default();
                immediate_results.push(result);
                continue;
            }

            let sync_secs = registration.implementation.sync_seconds();
            let timeout_secs = registration.implementation.timeout_seconds();

            let implementation = registration.implementation.clone();
            let input = tc.input.clone();
            let task_id = tc.id.clone();

            let cancel_token = CancellationToken::new();
            let cancel = cancel_token.clone();
            let tasks_ref = self.tasks.clone();
            let mut tasks = tasks_ref.lock().await;

            let task_handle = tokio::spawn(async move {
                let result = tokio::select! {
                    r = implementation.execute(input) => r,
                    _ = cancel.cancelled() => Err(ToolError::Cancel),
                    _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => Err(ToolError::Timeout { seconds: timeout_secs }),
                };
                // Set tool_use_id at result construction time
                match result {
                    Ok(mut tool_result) => {
                        tool_result.tool_use_id = task_id;
                        Ok(tool_result)
                    }
                    Err(e) => Err(e),
                }
            });

            tasks.push(TaskEntry::with_notify(
                tc.id.clone(),
                task_handle,
                cancel_token,
                sync_secs,
                notify_tx.clone(),
            ));
        }

        let tasks_ref = self.tasks.clone();
        let mut tasks = tasks_ref.lock().await;

        let wait_results = join_all(tasks.iter_mut().map(|t| t.wait())).await;

        let mut results: Vec<ToolResult> = wait_results
            .into_iter()
            .map(|r| {
                let mut result: ToolResult = r.into();

                // NOTE: replace effect symbol with tokio::watch
                result.effects = effects_map.remove(&result.tool_use_id).unwrap_or_default();
                result
            })
            .collect();

        results.extend(immediate_results);

        // Clear finished tasks
        tasks.retain(|t| !t.is_read());

        Ok(results)
    }

    pub fn tools(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|r| r.definition.clone()).collect()
    }

    #[deprecated]
    /// Poll for completed background tasks. Returns `(tool_use_id, ok, content)`
    /// for each finished task and removes them from the internal task list.
    /// Failed tasks are treated as error results and also removed.
    pub async fn poll_completed_tasks(&self) -> Vec<(String, bool, String)> {
        let mut tasks = self.tasks.lock().await;
        let mut completed = Vec::new();
        tasks.retain(|entry| {
            match entry.status() {
                TaskStatus::Done(ref result) => {
                    completed.push((
                        result.tool_use_id.clone(),
                        !result.is_error.unwrap_or_default(),
                        result.text_content(),
                    ));
                    false
                }
                TaskStatus::Failed(ref err) => {
                    // Treat failure as an error result.
                    let id = entry.id().to_string();
                    completed.push((id, false, err.to_string()));
                    false
                }
                _ => true, // still running
            }
        });
        completed
    }

    /// Check whether any background tasks are still running.
    pub async fn has_background_tasks(&self) -> bool {
        let tasks = self.tasks.lock().await;
        !tasks.is_empty()
    }

    /// Return tool definitions, optionally restricted to a name whitelist.
    ///
    /// When `allowed` is `None`, behaves like [`tools`](Self::tools).
    /// When `Some(names)`, only tools whose name is in `names` are
    /// returned. Used by the skill system to limit which tools the LLM
    /// is offered during a given workflow step.
    pub fn tools_filtered(&self, allowed: Option<&[String]>) -> Vec<ToolDefinition> {
        match allowed {
            None => self.tools(),
            Some(names) => self
                .tools
                .iter()
                .filter(|(name, _)| names.iter().any(|n| n == name.as_str()))
                .map(|(_, r)| r.definition.clone())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::ToolFunction;
    use agentik_sdk::types::ToolEffect;
    use agentik_sdk::types::tools::{ToolBuilder, ToolUse};
    use async_trait::async_trait;
    use serde_json::{Value, json};

    use super::{ToolRegistration, Toolset};

    struct MockTool {
        result_text: String,
    }

    impl MockTool {
        fn new(text: &str) -> Self {
            Self {
                result_text: text.to_string(),
            }
        }
    }

    #[async_trait]
    impl ToolFunction for MockTool {
        type Input = Value;

        async fn execute(
            &self,
            _input: Value,
        ) -> Result<crate::tools::ToolResult, crate::tools::error::ToolError> {
            Ok(crate::tools::ToolResult::success(self.result_text.clone()))
        }
    }

    fn mock_registration(
        name: &str,
        description: &str,
        effects: Vec<ToolEffect>,
    ) -> ToolRegistration {
        ToolRegistration {
            definition: ToolBuilder::new(name, description)
                .parameter("reason", "string", "reason")
                .required("reason")
                .build(),
            implementation: std::sync::Arc::new(MockTool::new("mock result")),
            effects,
        }
    }

    #[tokio::test]
    async fn test_register_and_list_tools() {
        let mut toolset = Toolset::new();
        toolset
            .register(mock_registration("test_tool", "A test tool", vec![]))
            .unwrap();

        let tools = toolset.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "test_tool");
    }

    #[tokio::test]
    async fn test_execute_tool() {
        let mut toolset = Toolset::new();
        toolset
            .register(mock_registration("test_tool", "A test tool", vec![]))
            .unwrap();

        let tool_call = ToolUse {
            id: "tc1".to_string(),
            name: "test_tool".to_string(),
            input: json!({ "reason": "test" }),
        };

        let results = toolset.execute(&[tool_call], None, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_use_id, "tc1");
        assert!(results[0].effects.is_empty());
    }

    #[tokio::test]
    async fn test_execute_tool_with_effect() {
        let mut toolset = Toolset::new();
        toolset
            .register(mock_registration(
                "abort_task",
                "Abort current task",
                vec![ToolEffect::Abort],
            ))
            .unwrap();

        let tool_call = ToolUse {
            id: "tc2".to_string(),
            name: "abort_task".to_string(),
            input: json!({ "reason": "task aborted" }),
        };

        let results = toolset.execute(&[tool_call], None, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].effects, vec![ToolEffect::Abort]);
    }
}

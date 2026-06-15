use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

use super::DynToolFunction;
use super::error::ToolError;
use agentik_sdk::types::Tool as SdkTool;
use agentik_sdk::types::ToolCallResponse;
use agentik_sdk::types::ToolEffect;
use agentik_sdk::types::tools::{ToolResult, ToolUse};

pub struct ToolRegistration {
    pub definition: SdkTool,
    pub implementation: Box<dyn DynToolFunction>,
    pub effects: Vec<ToolEffect>,
}

impl ToolRegistration {
    pub fn new(
        definition: SdkTool,
        implementation: Box<dyn DynToolFunction>,
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
            implementation: Box::new(tool),
            effects,
        }
    }
}

pub struct Toolset {
    tools: HashMap<String, ToolRegistration>,
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

    pub async fn execute(&self, toolcalls: &[ToolUse]) -> Result<Vec<ToolCallResponse>, ToolError> {
        let mut results = Vec::with_capacity(toolcalls.len());
        for tc in toolcalls {
            let Some(registration) = self.tools.get(&tc.name) else {
                continue;
            };
            let effects = registration.effects.clone();

            // NOTE: Remove SDK side input validation
            //
            // if let Err(e) = registration.definition.validate_input(&tc.input) {
            //     let response = ToolResult::error(tc.id.clone(), e.to_string());
            //     results.push(response.into_call_response(effects));
            //     continue;
            // }
            if let Err(e) = registration.implementation.validate_input(&tc.input) {
                let response = ToolResult::error(tc.id.clone(), e.to_string());
                results.push(response.into_call_response(effects));
                continue;
            }

            let timeout_secs = registration.implementation.timeout_seconds();

            // Troubleshot Timeout error early
            let exec_result: Result<ToolResult, ToolError> = timeout(
                Duration::from_secs(timeout_secs),
                registration.implementation.execute(tc.input.clone()),
            )
            .await
            .unwrap_or_else(|_| {
                Err(ToolError::Timeout {
                    seconds: timeout_secs,
                })
            });

            let tool_result = match exec_result {
                Ok(mut r) => {
                    r.tool_use_id = tc.id.clone();
                    r
                }
                Err(e) => ToolResult::error(tc.id.clone(), e.to_string()),
            };

            results.push(tool_result.into_call_response(effects));
        }

        Ok(results)
    }

    pub fn tools(&self) -> Vec<SdkTool> {
        self.tools.values().map(|r| r.definition.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use crate::tools::ToolFunction;
    use agentik_sdk::types::ToolEffect;
    use agentik_sdk::types::tools::{ToolBuilder, ToolUse};

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
            Ok(crate::tools::ToolResult::success(
                "mock_id",
                self.result_text.clone(),
            ))
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
            implementation: Box::new(MockTool::new("mock result")),
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

        let results = toolset.execute(&[tool_call]).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_use_id, "tc1");
        assert!(results[0].effects.is_empty());
    }

    #[tokio::test]
    async fn test_execute_tool_with_effect() {
        let mut toolset = Toolset::new();
        toolset
            .register(mock_registration(
                "attempt_complete",
                "Complete current task",
                vec![ToolEffect::AttemptComplete],
            ))
            .unwrap();

        let tool_call = ToolUse {
            id: "tc2".to_string(),
            name: "attempt_complete".to_string(),
            input: json!({ "reason": "task done" }),
        };

        let results = toolset.execute(&[tool_call]).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].effects, vec![ToolEffect::AttemptComplete]);
    }
}

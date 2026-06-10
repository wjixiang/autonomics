pub use crate::tools::toolset::{ToolRegistration, Toolset};

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use crate::tools::ToolFunction;
    use agentik_types::ToolEffect;
    use agentik_types::tools::{ToolBuilder, ToolUse};

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
        async fn execute(
            &self,
            _input: Value,
        ) -> Result<crate::tools::ToolResult, Box<dyn std::error::Error + Send + Sync>> {
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

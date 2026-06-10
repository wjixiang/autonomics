use std::error::Error;

use serde_json::{Map, Value};
use agentik_types::{Tool, ToolEffect, ToolInputSchema, ToolResult};
use async_trait::async_trait;

#[async_trait]
pub trait ToolFunction: Send + Sync {
    async fn execute(
        &self,
        input: serde_json::Value,
    ) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>>;

    fn validate_input(
        &self,
        _input: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn timeout_seconds(&self) -> u64 {
        30
    }

    fn definition(&self) -> Tool {
        Tool {
            name: std::any::type_name::<Self>().split("::").last().unwrap_or("unknown").to_string(),
            description: String::new(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Map::new(),
                required: vec![],
                additional: Map::new(),
            },
        }
    }

    fn effects(&self) -> Vec<ToolEffect> {
        vec![]
    }
}

/// Simple function wrapper for tool functions.
///
/// This allows you to register simple async functions as tools without
/// implementing the full `ToolFunction` trait.
pub struct SimpleTool<F>
where
    F: Fn(
            Value,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<ToolResult, Box<dyn Error + Send + Sync>>>
                    + Send,
            >,
        > + Send
        + Sync,
{
    function: F,
}

impl<F> SimpleTool<F>
where
    F: Fn(
            Value,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<ToolResult, Box<dyn Error + Send + Sync>>>
                    + Send,
            >,
        > + Send
        + Sync,
{
    /// Create a new simple tool from a function.
    pub fn new(function: F) -> Self {
        Self { function }
    }
}

#[async_trait]
impl<F> ToolFunction for SimpleTool<F>
where
    F: Fn(
            Value,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<ToolResult, Box<dyn Error + Send + Sync>>>
                    + Send,
            >,
        > + Send
        + Sync,
{
    async fn execute(&self, input: Value) -> Result<ToolResult, Box<dyn Error + Send + Sync>> {
        (self.function)(input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ToolResultContent;
    use async_trait::async_trait;
    use serde_json::json;

    struct TestEchoTool;

    #[async_trait]
    impl ToolFunction for TestEchoTool {
        async fn execute(
            &self,
            input: serde_json::Value,
        ) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>> {
            let message = input["message"].as_str().unwrap_or("No message");
            Ok(ToolResult::success("test_id", format!("Echo: {}", message)))
        }
    }

    #[tokio::test]
    async fn test_tool_function_execution() {
        let tool = TestEchoTool;
        let input = json!({"message": "Hello, World!"});
        let result = tool.execute(input).await.unwrap();
        if let ToolResultContent::Text(content) = result.content {
            assert_eq!(content, "Echo: Hello, World!");
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_simple_tool() {
        let tool = SimpleTool::new(|input: Value| {
            Box::pin(async move {
                let number = input["number"].as_f64().unwrap_or(0.0);
                let result = number * 2.0;
                Ok(ToolResult::success(
                    "test_id",
                    format!("Result: {}", result),
                ))
            })
        });

        let input = json!({"number": 21.0});
        let result = tool.execute(input).await.unwrap();
        if let ToolResultContent::Text(content) = result.content {
            assert_eq!(content, "Result: 42");
        } else {
            panic!("Expected text content");
        }
    }
}

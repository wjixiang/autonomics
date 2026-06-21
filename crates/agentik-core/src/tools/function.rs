use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::error::ToolError;
use agentik_sdk::types::{ToolDefinition, ToolEffect, ToolInput, ToolResult};

/// A tool that the agent can invoke.
///
/// Tools receive their inputs as a strongly-typed `Input` struct rather
/// than a raw [`serde_json::Value`]. The framework takes care of the
/// `Value -> Self::Input` conversion at the trait boundary; tool
/// implementations override [`run`](Self::run) and receive the
/// deserialized struct directly.
///
/// ## Choosing `Input`
///
/// - For tools with structured parameters, define a `#[derive(Deserialize)]`
///   struct and use it as `type Input`. Required fields become required
///   JSON fields; `Option<T>` fields default to `None`; `#[serde(default)]`
///   makes a field optional with the type's default value.
/// - For tools that take no input, use `type Input = serde_json::Value`
///   and override [`execute`](Self::execute) directly (skipping the
///   deserialize step).
///
/// ## Implementing a tool
///
/// Override [`run`](Self::run) to receive typed input:
///
/// ```ignore
/// #[derive(Deserialize)]
/// struct MyInput { name: String }
///
/// #[async_trait]
/// impl ToolFunction for MyTool {
///     type Input = MyInput;
///     async fn run(&self, input: MyInput) -> Result<ToolResult, ToolError> {
///         Ok(ToolResult::success(format!("hi {}", input.name)))
///     }
/// }
/// ```
///
/// Override [`execute`](Self::execute) instead when you need raw
/// `Value` access (e.g. to accept arbitrary extra fields).
///
/// ## Storage
///
/// `ToolFunction` has an associated type (`Input`), so `dyn ToolFunction`
/// cannot hold tools with different `Input` types. To store tools
/// heterogeneously (registry / toolset), erase to
/// [`DynToolFunction`]: every `T: ToolFunction` implements it via a
/// blanket impl, so storage sites can hold `Box<dyn DynToolFunction>`
/// while concrete call sites keep full type information.
#[async_trait]
pub trait ToolFunction: Send + Sync {
    /// Strongly-typed input parameter struct. See trait docs.
    ///
    /// The `ToolInput` bound means `Input` can describe its own
    /// [`ToolDefinition`] (typically derived via `#[derive(ToolInput)]`),
    /// which lets `definition()` delegate automatically.
    type Input: DeserializeOwned + ToolInput + Send + Sync;

    /// Framework entry point. Deserializes `input` into `Self::Input`
    /// and dispatches to [`run`](Self::run). Tool implementations
    /// should override `run` (typed) rather than this method unless
    /// they need to bypass the deserialize step.
    async fn execute(&self, input: Value) -> Result<ToolResult, ToolError> {
        let typed: Self::Input = serde_json::from_value(input)?;
        self.run(typed).await
    }

    /// Business implementation. Override this for typed input.
    ///
    /// Default panics — every concrete tool must override either
    /// `run` or `execute`.
    async fn run(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
        unimplemented!("override `run` (typed input) or `execute` (raw Value)")
    }

    fn validate_input(&self, _input: &Value) -> Result<(), ToolError> {
        Ok(())
    }

    fn timeout_seconds(&self) -> u64 {
        30
    }

    fn definition(&self) -> ToolDefinition {
        Self::Input::definition()
    }

    fn effects(&self) -> Vec<ToolEffect> {
        vec![]
    }
}

/// Type-erased view of a tool, for heterogeneous storage.
///
/// `ToolFunction::Input` is an associated type, which makes
/// `Box<dyn ToolFunction>` incompatible with holding tools whose
/// `Input` types differ. Every `T: ToolFunction` automatically
/// implements `DynToolFunction` via a blanket impl, so callers can:
///
/// - keep full type info on the concrete side (impl `ToolFunction`
///   with a concrete `Input` struct),
/// - erase to `Box<dyn DynToolFunction>` only at the registry /
///   storage boundary.
#[async_trait]
pub trait DynToolFunction: Send + Sync {
    async fn execute(&self, input: Value) -> Result<ToolResult, ToolError>;

    fn validate_input(&self, input: &Value) -> Result<(), ToolError>;

    fn timeout_seconds(&self) -> u64;

    fn definition(&self) -> ToolDefinition;

    fn effects(&self) -> Vec<ToolEffect>;
}

#[async_trait]
impl<T: ToolFunction + ?Sized> DynToolFunction for T {
    async fn execute(&self, input: Value) -> Result<ToolResult, ToolError> {
        ToolFunction::execute(self, input).await
    }

    fn validate_input(&self, input: &Value) -> Result<(), ToolError> {
        ToolFunction::validate_input(self, input)
    }

    fn timeout_seconds(&self) -> u64 {
        ToolFunction::timeout_seconds(self)
    }

    fn definition(&self) -> ToolDefinition {
        ToolFunction::definition(self)
    }

    fn effects(&self) -> Vec<ToolEffect> {
        ToolFunction::effects(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentik_sdk::types::tools::ToolResultContent;
    use async_trait::async_trait;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Deserialize, Serialize, agentik_proc::ToolInput)]
    #[tool(name = "echo", description = "Echo test tool")]
    struct EchoInput {
        message: String,
    }

    struct TestEchoTool;

    #[async_trait]
    impl ToolFunction for TestEchoTool {
        type Input = EchoInput;

        async fn run(&self, input: EchoInput) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::success(
                format!("Echo: {}", input.message),
            ))
        }
    }

    #[tokio::test]
    async fn test_tool_function_execution() {
        let tool = TestEchoTool;
        let input = json!({"message": "Hello, World!"});
        let result = ToolFunction::execute(&tool, input).await.unwrap();
        if let ToolResultContent::Text(content) = result.content {
            assert_eq!(content, "Echo: Hello, World!");
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_tool_function_missing_required_field() {
        let tool = TestEchoTool;
        let input = json!({});
        let err = ToolFunction::execute(&tool, input).await.unwrap_err();
        // missing field surfaces as a deserialization error
        assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    }
}


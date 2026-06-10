use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

use super::error::ToolError;
use super::ToolFunction;
use agentik_types::Tool as SdkTool;
use agentik_types::ToolCallResponse;
use agentik_types::ToolEffect;
use agentik_types::tools::{ToolResult, ToolUse};

pub struct ToolRegistration {
    pub definition: SdkTool,
    pub implementation: Box<dyn ToolFunction>,
    pub effects: Vec<ToolEffect>,
}

impl ToolRegistration {
    pub fn new(
        definition: SdkTool,
        implementation: Box<dyn ToolFunction>,
        effects: Vec<ToolEffect>,
    ) -> Self {
        Self {
            definition,
            implementation,
            effects,
        }
    }
}

impl<T: ToolFunction + 'static> From<T> for ToolRegistration {
    fn from(tool: T) -> Self {
        let definition = tool.definition();
        let effects = tool.effects();
        Self {
            definition,
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
            return Err(ToolError::RegistryError { message: format!("Tool '{}' is already registered", name) });
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

    pub async fn execute(
        &self,
        toolcalls: &[ToolUse],
    ) -> Result<Vec<ToolCallResponse>, ToolError> {
        let mut results = Vec::with_capacity(toolcalls.len());
        for tc in toolcalls {
            let Some(registration) = self.tools.get(&tc.name) else {
                continue;
            };
            let effects = registration.effects.clone();

            if let Err(e) = registration.definition.validate_input(&tc.input) {
                let response = ToolResult::error(tc.id.clone(), e.to_string());
                results.push(response.into_call_response(effects));
                continue;
            }
            if let Err(e) = registration.implementation.validate_input(&tc.input) {
                let response = ToolResult::error(tc.id.clone(), e.to_string());
                results.push(response.into_call_response(effects));
                continue;
            }

            let timeout_secs = registration.implementation.timeout_seconds();
            let result = timeout(
                Duration::from_secs(timeout_secs),
                registration.implementation.execute(tc.input.clone()),
            )
            .await;

            let tool_result = match result {
                Ok(Ok(mut r)) => {
                    r.tool_use_id = tc.id.clone();
                    r
                }
                Ok(Err(e)) => ToolResult::error(tc.id.clone(), e.to_string()),
                Err(_) => ToolResult::error(
                    tc.id.clone(),
                    format!("Tool execution timed out after {} seconds", timeout_secs),
                ),
            };

            results.push(tool_result.into_call_response(effects));
        }

        Ok(results)
    }

    pub fn tools(&self) -> Vec<SdkTool> {
        self.tools.values().map(|r| r.definition.clone()).collect()
    }
}

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, )]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: ToolInputSchema,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, )]
pub struct ToolInputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: Map<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(flatten)]
    pub additional: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolChoice {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "any")]
    Any,
    #[serde(rename = "tool")]
    Tool {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, )]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: ToolResultContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Json(Value),
    Blocks(Vec<ToolResultBlock>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolResultBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
    },
    #[serde(rename = "image")]
    Image {
        source: ImageSource,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, )]
#[serde(tag = "type")]
pub enum ImageSource {
    #[serde(rename = "base64")]
    Base64 {
        media_type: String,
        data: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, )]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    AttemptComplete,
    Abort,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolCallResponseContent {
    Text(String),
    Image(ImageSource),
}

#[derive(Debug, Clone, Serialize, Deserialize, )]
pub struct ToolCallResponse {
    pub tool_use_id: String,
    pub content: Vec<ToolCallResponseContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<ToolEffect>,
}



impl ToolResult {
    pub fn into_call_response(self, effects: Vec<ToolEffect>) -> ToolCallResponse {
        let content = match self.content {
            ToolResultContent::Text(s) => vec![ToolCallResponseContent::Text(s)],
            ToolResultContent::Json(v) => vec![ToolCallResponseContent::Text(v.to_string())],
            ToolResultContent::Blocks(blocks) => blocks
                .into_iter()
                .map(|b| match b {
                    ToolResultBlock::Text { text } => ToolCallResponseContent::Text(text),
                    ToolResultBlock::Image { source } => ToolCallResponseContent::Image(source),
                })
                .collect(),
        };

        ToolCallResponse {
            tool_use_id: self.tool_use_id,
            content,
            is_error: self.is_error,
            effects,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolBuilder {
    name: String,
    description: String,
    properties: Map<String, Value>,
    required: Vec<String>,
    additional: Map<String, Value>,
}

impl ToolBuilder {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            properties: Map::new(),
            required: Vec::new(),
            additional: Map::new(),
        }
    }

    pub fn parameter(
        mut self,
        name: impl Into<String>,
        param_type: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let param_name = name.into();
        let param_schema = serde_json::json!({
            "type": param_type.into(),
            "description": description.into()
        });
        self.properties.insert(param_name, param_schema);
        self
    }

    pub fn enum_parameter(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        values: Vec<String>,
    ) -> Self {
        let param_name = name.into();
        let param_schema = serde_json::json!({
            "type": "string",
            "description": description.into(),
            "enum": values
        });
        self.properties.insert(param_name, param_schema);
        self
    }

    pub fn array_parameter(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        item_type: impl Into<String>,
    ) -> Self {
        let param_name = name.into();
        let param_schema = serde_json::json!({
            "type": "array",
            "description": description.into(),
            "items": {
                "type": item_type.into()
            }
        });
        self.properties.insert(param_name, param_schema);
        self
    }

    pub fn object_parameter(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        properties: Map<String, Value>,
    ) -> Self {
        let param_name = name.into();
        let param_schema = serde_json::json!({
            "type": "object",
            "description": description.into(),
            "properties": properties
        });
        self.properties.insert(param_name, param_schema);
        self
    }

    pub fn required(mut self, name: impl Into<String>) -> Self {
        let param_name = name.into();
        if !self.required.contains(&param_name) {
            self.required.push(param_name);
        }
        self
    }

    pub fn additional_property(mut self, key: impl Into<String>, value: Value) -> Self {
        self.additional.insert(key.into(), value);
        self
    }

    pub fn build(self) -> ToolDefinition {
        ToolDefinition {
            name: self.name,
            description: self.description,
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: self.properties,
                required: self.required,
                additional: self.additional,
            },
        }
    }
}

impl ToolDefinition {
    pub fn builder() -> ToolBuilder {
        ToolBuilder {
            name: String::new(),
            description: String::new(),
            properties: Map::new(),
            required: Vec::new(),
            additional: Map::new(),
        }
    }

    pub fn validate_input(&self, input: &Value) -> Result<(), ToolValidationError> {
        if let Value::Object(input_obj) = input {
            for required_field in &self.input_schema.required {
                if !input_obj.contains_key(required_field) {
                    return Err(ToolValidationError::MissingRequiredField {
                        field: required_field.clone(),
                        tool: self.name.clone(),
                    });
                }
            }

            for (field_name, field_value) in input_obj {
                if let Some(property_schema) = self.input_schema.properties.get(field_name) {
                    self.validate_field_type(field_name, field_value, property_schema)?;
                }
            }

            Ok(())
        } else {
            Err(ToolValidationError::InvalidInputType {
                expected: "object".to_string(),
                actual: input.to_string(),
                tool: self.name.clone(),
            })
        }
    }

    fn validate_field_type(
        &self,
        field_name: &str,
        value: &Value,
        schema: &Value,
    ) -> Result<(), ToolValidationError> {
        if let Some(expected_type) = schema.get("type").and_then(|t| t.as_str()) {
            let actual_type = match value {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };

            if expected_type != actual_type {
                return Err(ToolValidationError::InvalidFieldType {
                    field: field_name.to_string(),
                    expected: expected_type.to_string(),
                    actual: actual_type.to_string(),
                    tool: self.name.clone(),
                });
            }
        }

        Ok(())
    }
}

impl ToolChoice {
    pub fn auto() -> Self {
        Self::Auto
    }

    pub fn any() -> Self {
        Self::Any
    }

    pub fn tool(name: impl Into<String>) -> Self {
        Self::Tool { name: name.into() }
    }
}

impl ToolResult {
    pub fn success(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: ToolResultContent::Text(content.into()),
            is_error: None,
        }
    }

    pub fn success_json(tool_use_id: impl Into<String>, content: Value) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: ToolResultContent::Json(content),
            is_error: None,
        }
    }

    pub fn error(tool_use_id: impl Into<String>, error_message: impl Into<String>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: ToolResultContent::Text(error_message.into()),
            is_error: Some(true),
        }
    }

    pub fn with_blocks(tool_use_id: impl Into<String>, blocks: Vec<ToolResultBlock>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: ToolResultContent::Blocks(blocks),
            is_error: None,
        }
    }
}

impl ToolResultBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image_base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self::Image {
            source: ImageSource::Base64 {
                media_type: media_type.into(),
                data: data.into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ToolValidationError {
    #[error("Missing required field '{field}' for tool '{tool}'")]
    MissingRequiredField { field: String, tool: String },
    #[error("Invalid input type for tool '{tool}': expected {expected}, got {actual}")]
    InvalidInputType {
        expected: String,
        actual: String,
        tool: String,
    },
    #[error("Invalid type for field '{field}' in tool '{tool}': expected {expected}, got {actual}")]
    InvalidFieldType {
        field: String,
        expected: String,
        actual: String,
        tool: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerTool {
    #[serde(rename = "web_search_20250305")]
    WebSearch {
        #[serde(skip_serializing_if = "Option::is_none")]
        parameters: Option<WebSearchParameters>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_results: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<String>,
}

impl ServerTool {
    pub fn web_search() -> Self {
        Self::WebSearch { parameters: None }
    }

    pub fn web_search_with_params(parameters: WebSearchParameters) -> Self {
        Self::WebSearch {
            parameters: Some(parameters),
        }
    }
}

impl WebSearchParameters {
    pub fn with_max_results(max_results: u32) -> Self {
        Self {
            max_results: Some(max_results),
            language: None,
            region: None,
        }
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_builder() {
        let tool = ToolBuilder::new("get_weather", "Get the current weather")
            .parameter("location", "string", "The location to get weather for")
            .parameter("unit", "string", "Temperature unit")
            .enum_parameter(
                "format",
                "Response format",
                vec!["json".to_string(), "text".to_string()],
            )
            .required("location")
            .build();

        assert_eq!(tool.name, "get_weather");
        assert_eq!(tool.description, "Get the current weather");
        assert_eq!(tool.input_schema.required, vec!["location"]);
        assert_eq!(tool.input_schema.properties.len(), 3);
    }

    #[test]
    fn test_tool_validation() {
        let tool = ToolBuilder::new("test_tool", "Test tool")
            .parameter("required_field", "string", "Required field")
            .parameter("optional_field", "number", "Optional field")
            .required("required_field")
            .build();

        let valid_input = json!({
            "required_field": "test",
            "optional_field": 42
        });
        assert!(tool.validate_input(&valid_input).is_ok());

        let invalid_input = json!({
            "optional_field": 42
        });
        assert!(tool.validate_input(&invalid_input).is_err());

        let wrong_type_input = json!({
            "required_field": 123
        });
        assert!(tool.validate_input(&wrong_type_input).is_err());
    }

    #[test]
    fn test_tool_choice_serialization() {
        let auto_choice = ToolChoice::auto();
        let json = serde_json::to_value(&auto_choice).unwrap();
        assert_eq!(json, json!({"type": "auto"}));

        let tool_choice = ToolChoice::tool("get_weather");
        let json = serde_json::to_value(&tool_choice).unwrap();
        assert_eq!(json, json!({"type": "tool", "name": "get_weather"}));
    }

    #[test]
    fn test_tool_result_creation() {
        let success_result = ToolResult::success("tool_123", "Success message");
        assert_eq!(success_result.tool_use_id, "tool_123");
        assert!(success_result.is_error.is_none());

        let error_result = ToolResult::error("tool_456", "Error message");
        assert_eq!(error_result.tool_use_id, "tool_456");
        assert_eq!(error_result.is_error, Some(true));

        let json_result = ToolResult::success_json("tool_789", json!({"temperature": 72}));
        if let ToolResultContent::Json(value) = json_result.content {
            assert_eq!(value["temperature"], 72);
        } else {
            panic!("Expected JSON content");
        }
    }

    #[test]
    fn test_server_tool_creation() {
        let web_search = ServerTool::web_search();
        assert!(matches!(
            web_search,
            ServerTool::WebSearch { parameters: None }
        ));

        let params = WebSearchParameters::with_max_results(10)
            .language("en")
            .region("US");
        let web_search_with_params = ServerTool::web_search_with_params(params);

        if let ServerTool::WebSearch {
            parameters: Some(p),
        } = web_search_with_params
        {
            assert_eq!(p.max_results, Some(10));
            assert_eq!(p.language, Some("en".to_string()));
            assert_eq!(p.region, Some("US".to_string()));
        } else {
            panic!("Expected web search with parameters");
        }
    }

    #[test]
    fn test_tool_serialization() {
        let tool = ToolBuilder::new("calculate", "Perform mathematical calculations")
            .parameter(
                "expression",
                "string",
                "Mathematical expression to evaluate",
            )
            .required("expression")
            .build();

        let json = serde_json::to_string(&tool).unwrap();
        let deserialized: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, deserialized);
    }

    #[test]
    fn test_tool_use_deserialization() {
        let json = r#"
        {
            "id": "toolu_123456",
            "name": "get_weather",
            "input": {
                "location": "San Francisco, CA",
                "unit": "celsius"
            }
        }"#;

        let tool_use: ToolUse = serde_json::from_str(json).unwrap();
        assert_eq!(tool_use.id, "toolu_123456");
        assert_eq!(tool_use.name, "get_weather");
        assert_eq!(tool_use.input["location"], "San Francisco, CA");
        assert_eq!(tool_use.input["unit"], "celsius");
    }
}

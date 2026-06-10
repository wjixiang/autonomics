use std::sync::Arc;

use crate::model::model_info::ModelInfo;
use crate::provider::client::ApiClient;
use crate::streaming::MessageStream;
use agentik_types::errors::AnthropicError;
use agentik_types::messages::Message;
use agentik_types::tools::ToolDefinition;

pub struct Model {
    pub model_info: ModelInfo,
    client: Arc<dyn ApiClient>,
}

impl Model {
    pub fn new(model_info: ModelInfo, client: impl ApiClient + 'static) -> Self {
        Self {
            model_info,
            client: Arc::new(client),
        }
    }
    pub fn vision(mut self, enabled: bool) -> Self {
        self.model_info.vision_ability = enabled;
        self
    }
    pub fn set_context_window(mut self, window: u64) -> Self {
        self.model_info.context_length = window;
        self
    }

    pub fn context_length(&self) -> u64 {
        self.model_info.context_length
    }

    pub async fn request(
        &self,
        messages: Vec<Message>,
        tools: &[ToolDefinition],
    ) -> Result<Message, AnthropicError> {
        let response = self
            .client
            .request(messages, tools.to_vec(), &self.model_info)
            .await?;
        Ok(response)
    }

    pub async fn request_stream(
        &self,
        messages: Vec<Message>,
        tools: &[ToolDefinition],
    ) -> Result<MessageStream, AnthropicError> {
        self.client
            .request_stream(messages, tools.to_vec(), &self.model_info)
            .await
    }
}

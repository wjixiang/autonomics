use crate::model::{Model, ModelInfo};
use crate::provider::client::AnthropicApiClient;
use crate::provider::{LlmProvider, ProviderError, ProviderInfo};
use async_trait::async_trait;
use crate::Anthropic;
use crate::config::ClientConfig;
use crate::config::LogLevel;
use crate::http::auth::AuthMethod;

pub const MODEL_MINIMAX_M2_7: &str = "MiniMax-M2.7";

pub struct MinimaxProvider {
    info: ProviderInfo,
}

impl MinimaxProvider {
    pub fn new(base_url: String, api_key: String) -> Self {
        let info = ProviderInfo {
            base_url,
            api_key,
            preset_models: Self::preset_models(),
        };

        Self { info }
    }

    pub fn preset_models() -> Vec<ModelInfo> {
        vec![ModelInfo {
            model_name: MODEL_MINIMAX_M2_7.to_string(),
            provider: "minimax".to_string(),
            context_length: 1_000_000,
            max_output_tokens: 1000,
            vision_ability: true,
            supports_function_calling: true,
            supports_streaming: true,
            supports_thinking: true,
            input_token_price: 4.0,
            output_token_price: 16.0,
        }]
    }

    fn build_client_config(&self) -> ClientConfig {
        ClientConfig {
            api_key: self.info.api_key.clone(),
            base_url: self.info.base_url.clone(),
            timeout: core::time::Duration::from_secs(30),
            max_retries: 3,
            log_level: LogLevel::Debug,
            auth_method: AuthMethod::Bearer,
        }
    }
}

#[async_trait]
impl LlmProvider for MinimaxProvider {
    fn get_model(&self, model_name: &str) -> Result<Model, ProviderError> {
        let existed_model = self
            .info
            .preset_models
            .iter()
            .find(|i| i.model_name == model_name)
            .ok_or_else(|| {
                ProviderError::ModelNotFound(ModelInfo {
                    model_name: model_name.to_string(),
                    provider: "minimax".to_string(),
                    ..Default::default()
                })
            })?;

        let client = AnthropicApiClient::new(Anthropic::with_config(self.build_client_config())?);
        Ok(Model::new(existed_model.clone(), client))
    }

    fn add_models(&mut self, model: Vec<ModelInfo>) {
        self.info.preset_models.extend(model);
    }

    async fn list_models(&self) -> Result<Vec<Model>, ProviderError> {
        let client = Anthropic::with_config(self.build_client_config())?;
        let model_list = client.models().list(None).await?;

        let mut models = Vec::with_capacity(model_list.data.len());
        for model_obj in &model_list.data {
            if let Some(model_info) = self
                .info
                .preset_models
                .iter()
                .find(|i| i.model_name == model_obj.id)
            {
                let api_client =
                    AnthropicApiClient::new(Anthropic::with_config(self.build_client_config())?);
                models.push(Model::new(model_info.clone(), api_client));
            }
        }
        Ok(models)
    }
}

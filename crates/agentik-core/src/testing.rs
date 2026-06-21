use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::model::{Model, ModelInfo};
use agentik_sdk::provider::client::MockApiClient;
use agentik_sdk::http::auth::AuthMethod;

pub fn dummy_model_info(name: &str) -> ModelInfo {
    ModelInfo {
        model_name: name.into(),
        provider_name: "test".into(),
        context_length: 4096,
        max_output_tokens: 1024,
        vision_ability: true,
        supports_function_calling: true,
        supports_streaming: true,
        supports_thinking: false,
        input_token_price: 1.0,
        output_token_price: 2.0,
        base_url: String::new(),
        api_key: String::new(),
        auth_method: AuthMethod::default(),
    }
}

pub fn get_mock_model_pool(dummy_model_name: &str) -> ModelPool {
    let mut model_pool = ModelPool::new();
    let mock_model = Model::with_client(dummy_model_info(dummy_model_name), MockApiClient::new());
    model_pool.add_model(mock_model);
    model_pool
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum AiProvider {
    API,
    CMD,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AiClientConfig {
    pub provider: AiProvider,
    pub api_key: String,
    pub api_url: String,
    pub api_model: String,
    pub cmd: String,
    pub prompt: String,
}

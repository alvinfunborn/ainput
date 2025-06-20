use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AiClientConfig {
    pub api_key: String,
    pub url: String,
    pub model: String,
    pub prompt: String,
}
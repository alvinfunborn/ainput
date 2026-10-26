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
    /// Empty uses environment / OS proxy settings; otherwise an HTTP(S) proxy URL.
    #[serde(default)]
    pub api_proxy: String,
    pub cmd: String,
    pub prompt: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_config_without_proxy_defaults_to_automatic() {
        let config: AiClientConfig = serde_json::from_value(serde_json::json!({
            "provider": "API", "api_key": "", "api_url": "https://example.invalid", "api_model": "test", "cmd": "", "prompt": ""
        })).unwrap();
        assert!(config.api_proxy.is_empty());
    }
}

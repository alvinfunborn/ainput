use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PrivacyConfig {
    pub enable: bool,
    pub rules: Vec<String>,
} 
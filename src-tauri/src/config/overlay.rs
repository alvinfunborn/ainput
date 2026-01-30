use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OverlayConfig {
    pub refresh_interval: u64,
    pub relative_x: i32,
    pub relative_y: i32,
    pub style: String,
    /// AI 返回空结果时，重试的最小间隔/ms（防止无限循环）
    #[serde(default = "default_retry_interval")]
    pub retry_interval: u64,
}

fn default_retry_interval() -> u64 {
    3000
}

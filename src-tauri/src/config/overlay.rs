use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OverlayConfig {
    pub refresh_interval: u64,
    pub relative_x: i32,
    pub relative_y: i32,
    pub style: String,
    /// Legacy setting retained for config compatibility. Empty responses wait for new input.
    #[serde(default = "default_retry_interval")]
    pub retry_interval: u64,
}

fn default_retry_interval() -> u64 {
    3000
}

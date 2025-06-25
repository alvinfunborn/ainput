use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OverlayConfig {
    pub refresh_interval: u64,
    pub relative_x: i32,
    pub relative_y: i32,
    pub style: String,
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SystemConfig {
    pub show_tray_icon: bool,
    pub start_at_login: bool,
    pub logging_level: String,
    pub refresh_overlay_interval: u64,
    pub overlay_relative_x: i64,
    pub overlay_relative_y: i64,
    pub overlay_style: String,
    pub history_ttl: u64,
}

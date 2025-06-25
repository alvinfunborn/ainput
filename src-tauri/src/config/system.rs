use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SystemConfig {
    pub show_tray_icon: bool,
    pub start_at_login: bool,
    pub logging_level: String,
    pub history_ttl: u64,
}

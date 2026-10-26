use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SystemConfig {
    #[serde(default = "default_enabled")]
    pub completion_enabled: bool,
    pub show_tray_icon: bool,
    pub start_at_login: bool,
    pub logging_level: String,
    pub history_ttl: u64,
}

fn default_enabled() -> bool { true }

#[cfg(test)]
mod tests {
    #[test]
    fn existing_settings_keep_completion_enabled() {
        let config: super::SystemConfig = toml::from_str(
            "show_tray_icon=true\nstart_at_login=false\nlogging_level='info'\nhistory_ttl=1000"
        ).unwrap();
        assert!(config.completion_enabled);
    }
}

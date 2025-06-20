use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UiAutomationConfig {
    pub collect_interval: u64,
    pub ignore_apps: Vec<String>,
    pub default_edit_control_types: Vec<i32>,
    pub hastext_edit_control_types: Vec<i32>,
    pub app_edit_control_types: HashMap<String, Vec<i32>>,
}

pub mod system;
pub mod ui_automation;
pub mod keyboard;
pub mod ai_client;
pub mod keybinding;
pub mod privacy;
pub mod overlay;

use log::{debug, error, info};
pub use system::SystemConfig;
pub use ui_automation::UiAutomationConfig;
pub use ai_client::AiClientConfig;
pub use keybinding::KeybindingConfig;
pub use privacy::PrivacyConfig;
pub use overlay::OverlayConfig;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use toml;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub system: SystemConfig,
    pub ui_automation: UiAutomationConfig,
    pub keyboard: KeyboardConfig,
    pub ai_client: AiClientConfig,
    pub keybinding: KeybindingConfig,
    pub privacy: PrivacyConfig,
    pub overlay: OverlayConfig,
}

pub fn get_config_path() -> Option<String> {
    let config_paths = vec!["config.toml", "src-tauri/config.toml", "../config.toml"];
    for path in config_paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

pub fn load_config() -> Config {
    if let Some(path) = get_config_path() {
        let config_str = fs::read_to_string(&path)
            .expect(format!("[load_config] Failed to read config file: {}", path).as_str());
        let config: Config = toml::from_str(&config_str)
            .expect(format!("[load_config] Failed to parse config file: {}", path).as_str());
        info!("[load_config] load config from{} : {:?}", path, config);
        return config;
    }
    panic!("please check the config file: config.toml exists");
}

// 全局配置实例
use once_cell::sync::Lazy;
use std::sync::Mutex;

use crate::config::keyboard::KeyboardConfig;

pub static CONFIG: Lazy<Mutex<Option<Config>>> = Lazy::new(|| Mutex::new(None));

// 初始化配置
pub fn init_config() -> Config {
    let config = load_config();
    let mut config_guard = CONFIG.lock().unwrap();
    *config_guard = Some(config.clone());
    config
}

// 获取配置
pub fn get_config() -> Option<Config> {
    CONFIG.lock().unwrap().clone()
}

// 为前端提供的配置获取命令
#[tauri::command]
pub fn get_config_for_frontend() -> Config {
    get_config().unwrap_or_else(|| {
        let config = load_config();
        let mut config_guard = CONFIG.lock().unwrap();
        *config_guard = Some(config.clone());
        config
    })
}

// 为前端提供的配置保存命令
#[tauri::command]
pub fn save_config_for_frontend(config: Config) {
    // 更新内存中的配置
    {
        let mut config_guard = CONFIG.lock().unwrap();
        *config_guard = Some(config.clone());
    }

    // 获取当前配置文件路径，如果不存在则使用默认路径
    let config_path = get_config_path().unwrap_or_else(|| {
        if cfg!(debug_assertions) {
            "src-tauri/config.toml".to_string()
        } else {
            "config.toml".to_string()
        }
    });

    // 确保目标目录存在
    if let Some(parent) = Path::new(&config_path).parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                error!(
                    "[save_config_for_frontend] Failed to create config directory: {}",
                    e
                );
            }
        }
    }

    match toml::to_string_pretty(&config) {
        Ok(config_str) => {
            if let Err(e) = fs::write(&config_path, config_str) {
                error!(
                    "[save_config_for_frontend] Failed to write config file: {}",
                    e
                );
            }
        }
        Err(e) => {
            error!(
                "[save_config_for_frontend] Failed to serialize config: {}",
                e
            );
        }
    }
}

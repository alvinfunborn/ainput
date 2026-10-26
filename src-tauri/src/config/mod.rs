pub mod ai_client;
pub mod keybinding;
pub mod keyboard;
pub mod overlay;
pub mod privacy;
pub mod system;
pub mod ui_automation;

pub use ai_client::AiClientConfig;
pub use keybinding::KeybindingConfig;
use log::info;
pub use overlay::OverlayConfig;
pub use privacy::PrivacyConfig;
pub use system::SystemConfig;
pub use ui_automation::UiAutomationConfig;

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
    #[cfg(target_os = "macos")]
    {
        return Some(
            crate::paths::data_dir()
                .join("config.toml")
                .to_string_lossy()
                .into_owned(),
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let config_paths = vec!["config.toml", "src-tauri/config.toml", "../config.toml"];
        for path in config_paths {
            if Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
        None
    }
}

pub fn load_config() -> Config {
    if let Some(path) = get_config_path() {
        let config_str = fs::read_to_string(&path)
            .expect(format!("[load_config] Failed to read config file: {}", path).as_str());
        let config: Config = toml::from_str(&config_str)
            .expect(format!("[load_config] Failed to parse config file: {}", path).as_str());
        info!("[load_config] loaded config from {}", path);
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

// Persist first, so a failed save never claims success or changes the running config.
#[tauri::command]
pub fn save_config_for_frontend(config: Config) -> Result<(), String> {
    let mut current = CONFIG.lock().map_err(|_| "配置锁不可用，请重启 AInput")?;
    let path = get_config_path().ok_or("找不到配置文件")?;
    write_config(Path::new(&path), &config).map_err(|e| format!("保存失败：{e}"))?;
    *current = Some(config);
    drop(current);
    crate::input::sync_completion_enabled();
    Ok(())
}

// Persist only this switch, preserving any unsaved settings draft in the UI.
#[tauri::command]
pub fn set_completion_enabled(enabled: bool) -> Result<bool, String> {
    let mut current = CONFIG.lock().map_err(|_| "配置锁不可用，请重启 AInput")?;
    let mut next = current.clone().ok_or("设置尚未加载")?;
    next.system.completion_enabled = enabled;
    let path = get_config_path().ok_or("找不到配置文件")?;
    write_config(Path::new(&path), &next).map_err(|e| format!("保存失败：{e}"))?;
    *current = Some(next);
    drop(current);
    crate::input::sync_completion_enabled();
    Ok(crate::input::completion_enabled())
}

fn write_config(path: &Path, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let serialized = toml::to_string_pretty(config)?;
    let parent = path.parent().ok_or("Missing config directory")?;
    fs::create_dir_all(parent)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary = parent.join(format!(".ainput-config-{}-{stamp}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(serialized.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(())
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    #[test]
    fn atomic_save_replaces_config_and_reports_write_failure() {
        let directory =
            std::env::temp_dir().join(format!("ainput-save-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.toml");
        let mut config: Config = toml::from_str(include_str!("../../config_macos.toml")).unwrap();
        write_config(&path, &config).unwrap();
        config.ai_client.api_model = "changed-model".into();
        config.system.completion_enabled = false;
        write_config(&path, &config).unwrap();
        let saved: Config = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.ai_client.api_model, "changed-model");
        assert!(!saved.system.completion_enabled);
        assert!(write_config(&path.join("invalid-child"), &config).is_err());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }
}

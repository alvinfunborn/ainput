#[cfg(target_os = "windows")]
pub mod app_element;
pub mod types;
#[cfg(target_os = "windows")]
pub mod ui_automation;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
pub mod ui_automation;
#[cfg(target_os = "windows")]
use log::debug;
pub use ui_automation::FocusedInput;

#[cfg(target_os = "windows")]
use crate::config;
use once_cell::sync::Lazy;
use std::sync::RwLock;
#[cfg(target_os = "windows")]
use std::thread;
#[cfg(target_os = "macos")]
pub use ui_automation::collect_input_focus;
#[cfg(target_os = "windows")]
use ui_automation::get_focused_input;

pub static CURRENT_FOCUS_INFO: Lazy<RwLock<Option<FocusedInput>>> = Lazy::new(|| RwLock::new(None));

#[cfg(target_os = "windows")]
pub fn collect_input_focus() {
    let collect_interval = config::get_config().unwrap().ui_automation.collect_interval;
    thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(collect_interval));

        let info = get_focused_input();
        let mut guard = CURRENT_FOCUS_INFO.write().unwrap();
        *guard = info;
    });
}

pub fn get_current_focus_info() -> Option<FocusedInput> {
    let guard = CURRENT_FOCUS_INFO.read().unwrap();
    guard.clone()
}

#[cfg(target_os = "windows")]
pub fn fill_input(selected_chars: String) {
    let focused_input = {
        let guard = CURRENT_FOCUS_INFO.read().unwrap();
        guard.clone()
    };
    if let Some(focused_input) = focused_input {
        ui_automation::fill_input(focused_input, selected_chars);
    } else {
        debug!("[fill_input] no focused input");
    }
}

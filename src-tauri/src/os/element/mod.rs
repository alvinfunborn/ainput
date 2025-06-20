pub mod ui_automation;
pub mod app_element;
use log::debug;
pub use ui_automation::FocusedInput;

use crate::config;
use once_cell::sync::Lazy;
use std::sync::RwLock;
use ui_automation::get_focused_input;
use std::thread;

pub static CURRENT_FOCUS_INFO: Lazy<RwLock<Option<FocusedInput>>> =
    Lazy::new(|| RwLock::new(None));

pub fn collect_input_focus() {
    let collect_interval = config::get_config().unwrap().ui_automation.collect_interval;
    thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(collect_interval));

            let info = get_focused_input();
            let mut guard = CURRENT_FOCUS_INFO.write().unwrap();
            *guard = info;
        }
    });
}

pub fn get_current_focus_info() -> Option<FocusedInput> {
    let guard = CURRENT_FOCUS_INFO.read().unwrap();
    guard.clone()
}

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

pub mod ui_automation;
pub mod app_element;
use log::info;
pub use ui_automation::UIElement;
pub use ui_automation::FocusedInput;

use crate::config;
use std::time::Duration;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use crate::os::WindowElement;
use ui_automation::get_focused_input;

pub static CURRENT_FOCUS_INFO: Lazy<Mutex<Option<FocusedInput>>> =
    Lazy::new(|| Mutex::new(None));

pub fn listen_input_focus() {
    std::thread::spawn(|| {
        loop {
            let info = get_focused_input();
            let mut guard = CURRENT_FOCUS_INFO.lock().unwrap();
            *guard = info;
            std::thread::sleep(std::time::Duration::from_millis(3000));
        }
    });
}

pub fn get_current_focus_info() -> Option<FocusedInput> {
    CURRENT_FOCUS_INFO.lock().unwrap().clone()
}

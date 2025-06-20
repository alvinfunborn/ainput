use std::{collections::{HashMap, HashSet}, sync::Mutex};

use log::debug;
use once_cell::sync::Lazy;

use crate::config;

struct KeyboardState {
    hold_keys: HashSet<String>,
}

impl KeyboardState {
    fn new() -> Self {
        Self { hold_keys: HashSet::new() }
    }
}

// 全局键盘状态
static KEYBOARD_STATE: Lazy<Mutex<KeyboardState>> =
    Lazy::new(|| Mutex::new(KeyboardState::new()));

pub fn handle_keyboard_event(app_handle: &tauri::AppHandle, key: &str, is_press: bool) -> bool {
    if !super::get_input_state() {
        return false;
    }
    debug!("[handle_keyboard_event] key: {}, is_down: {}", key, is_press);
    let config = config::get_config().unwrap();
    let mut state = KEYBOARD_STATE.lock().unwrap();
    if is_press {
        let propagation_modifier = config.keyboard.propagation_modifier;
        if propagation_modifier.contains(&key.to_string()) {
            state.hold_keys.insert(key.to_string());
        } else if !state.hold_keys.is_empty() {
            return false;
        }
    } else {
        state.hold_keys.remove(key);
        return false;
    }
    let keybinding_config = config::keybinding::get_keybinding_config();
    for (cmd, keys) in keybinding_config {
        if keys.contains(&key.to_string()) {
            match Some(cmd.as_str()) {
                Some(config::keybinding::EXIT_OVERLAY) => {
                    super::end_overlay();
                    return true;
                },
                Some(config::keybinding::ACCEPT_CANDIDATE) => {
                    super::select_candidate(-1);
                    return true;
                },
                Some(config::keybinding::SELECT_CANDIDATE_CHAR_1) => {
                    super::select_candidate(1);
                    return true;
                },
                Some(config::keybinding::SELECT_CANDIDATE_CHAR_2) => {
                    super::select_candidate(2);
                    return true;
                },
                Some(config::keybinding::SELECT_CANDIDATE_CHAR_3) => {
                    super::select_candidate(3);
                    return true;
                },
                Some(config::keybinding::SELECT_CANDIDATE_CHAR_4) => {
                    super::select_candidate(4);
                    return true;
                },
                Some(config::keybinding::SELECT_CANDIDATE_CHAR_5) => {
                    super::select_candidate(5);
                    return true;
                },
                Some(config::keybinding::SELECT_CANDIDATE_CHAR_6) => {
                    super::select_candidate(6);
                    return true;
                },
                Some(config::keybinding::SELECT_CANDIDATE_CHAR_7) => {
                    super::select_candidate(7);
                    return true;
                },
                Some(config::keybinding::SELECT_CANDIDATE_CHAR_8) => {
                    super::select_candidate(8);
                    return true;
                },
                Some(config::keybinding::SELECT_CANDIDATE_CHAR_9) => {
                    super::select_candidate(9);
                    return true;
                },
                _ => {
                    return false;
                }
            }
        }
    }
    false
}


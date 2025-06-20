use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct KeybindingConfig {
    pub exit_overlay: Vec<String>,
    pub accept_candidate: Vec<String>,
    pub select_candidate_char_1: Vec<String>,
    pub select_candidate_char_2: Vec<String>,
    pub select_candidate_char_3: Vec<String>,
    pub select_candidate_char_4: Vec<String>,
    pub select_candidate_char_5: Vec<String>,
    pub select_candidate_char_6: Vec<String>,
    pub select_candidate_char_7: Vec<String>,
    pub select_candidate_char_8: Vec<String>,
    pub select_candidate_char_9: Vec<String>,
}

pub const EXIT_OVERLAY: &str = "exit_overlay";
pub const ACCEPT_CANDIDATE: &str = "accept_candidate";
pub const SELECT_CANDIDATE_CHAR_1: &str = "select_candidate_char_1";
pub const SELECT_CANDIDATE_CHAR_2: &str = "select_candidate_char_2";
pub const SELECT_CANDIDATE_CHAR_3: &str = "select_candidate_char_3";
pub const SELECT_CANDIDATE_CHAR_4: &str = "select_candidate_char_4";
pub const SELECT_CANDIDATE_CHAR_5: &str = "select_candidate_char_5";
pub const SELECT_CANDIDATE_CHAR_6: &str = "select_candidate_char_6";
pub const SELECT_CANDIDATE_CHAR_7: &str = "select_candidate_char_7";
pub const SELECT_CANDIDATE_CHAR_8: &str = "select_candidate_char_8";
pub const SELECT_CANDIDATE_CHAR_9: &str = "select_candidate_char_9";

pub fn get_keybinding_config() -> HashMap<String, Vec<String>> {
    let config = super::get_config().unwrap();
    let keybinding_config = config.keybinding;
    let mut keybinding_map = HashMap::new();
    keybinding_map.insert(EXIT_OVERLAY.to_string(), keybinding_config.exit_overlay);
    keybinding_map.insert(ACCEPT_CANDIDATE.to_string(), keybinding_config.accept_candidate);
    keybinding_map.insert(SELECT_CANDIDATE_CHAR_1.to_string(), keybinding_config.select_candidate_char_1);
    keybinding_map.insert(SELECT_CANDIDATE_CHAR_2.to_string(), keybinding_config.select_candidate_char_2);
    keybinding_map.insert(SELECT_CANDIDATE_CHAR_3.to_string(), keybinding_config.select_candidate_char_3);
    keybinding_map.insert(SELECT_CANDIDATE_CHAR_4.to_string(), keybinding_config.select_candidate_char_4);
    keybinding_map.insert(SELECT_CANDIDATE_CHAR_5.to_string(), keybinding_config.select_candidate_char_5);
    keybinding_map.insert(SELECT_CANDIDATE_CHAR_6.to_string(), keybinding_config.select_candidate_char_6);
    keybinding_map.insert(SELECT_CANDIDATE_CHAR_7.to_string(), keybinding_config.select_candidate_char_7);
    keybinding_map.insert(SELECT_CANDIDATE_CHAR_8.to_string(), keybinding_config.select_candidate_char_8);
    keybinding_map.insert(SELECT_CANDIDATE_CHAR_9.to_string(), keybinding_config.select_candidate_char_9);
    keybinding_map
}
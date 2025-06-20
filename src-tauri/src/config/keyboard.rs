use std::sync::Mutex;

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct KeyboardConfig {
    pub available_key: IndexMap<String, u16>,
    pub propagation_modifier: Vec<String>,
}

pub static VIRTUAL_KEY_MAP: Lazy<Mutex<IndexMap<u16, String>>> = Lazy::new(|| Mutex::new({
    let mut map = IndexMap::new();
    for (key, vk) in &super::get_config().unwrap().keyboard.available_key {
        map.insert(*vk, key.clone());
    }
    map
}));


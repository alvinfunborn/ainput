use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowElement {
    pub id: i64,
    pub app: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub title: String,
    pub class_name: String,
}

impl Hash for WindowElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for WindowElement {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for WindowElement {}

#[cfg(target_os = "windows")]
pub mod windows_window;
#[cfg(target_os = "windows")]
pub use windows_window::get_current_window;

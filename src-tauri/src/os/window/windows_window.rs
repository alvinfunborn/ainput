use crate::utils::Rect;
use indexmap::IndexMap;
use log::{debug, error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::ptr;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetClientRect, GetForegroundWindow, GetTopWindow, GetWindow, GetWindowLongW, GetWindowRect, GetWindowTextW, IsIconic, IsWindowVisible, GWL_EXSTYLE, GW_HWNDNEXT, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowElement {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub title: String,
    pub class_name: String,
    pub window_handle: i64,
}

impl Hash for WindowElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.window_handle.hash(state);
    }
}

impl PartialEq for WindowElement {
    fn eq(&self, other: &Self) -> bool {
        self.window_handle == other.window_handle
    }
}

impl Eq for WindowElement {}

pub fn get_current_window() -> Option<WindowElement> {
    unsafe {
        // 1. 获取前台窗口句柄
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            error!("[get_focused_input_and_relative_rect] No foreground window");
            return None;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_ok() && IsWindowVisible(hwnd).as_bool() {
            let mut title = [0u16; 512];
            let mut class_name = [0u16; 512];

            GetWindowTextW(hwnd, &mut title);
            GetClassNameW(hwnd, &mut class_name);

            let title =
                String::from_utf16_lossy(&title[..title.iter().position(|&x| x == 0).unwrap_or(0)]);
            let class_name = String::from_utf16_lossy(
                &class_name[..class_name.iter().position(|&x| x == 0).unwrap_or(0)],
            );

            let window_element = WindowElement {
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
                title: title.clone(),
                class_name: class_name.clone(),
                window_handle: hwnd.0 as i64,
            };
            Some(window_element)
        } else {
            None
        }
    }
}

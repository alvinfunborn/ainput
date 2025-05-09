use log::{debug, error, info};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use windows::Win32::{Foundation::*, System::Com::*, UI::Accessibility::*};
use std::time::{SystemTime, UNIX_EPOCH};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::core::Interface;

use crate::{config, os::{window, WindowElement}};

#[derive(Clone, Debug)]
pub struct UIElement {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub width: i32,
    pub height: i32,
    pub window_handle: i64,
    pub control_type: i32,
    // element_type: 0-default, 1-window, 2-pane, 3-tab, 4-button, 5-scrollbar
    pub element_type: usize,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct FocusedInput {
    pub window_element: WindowElement,
    pub input_element: UIElement,
}

impl Eq for FocusedInput {}

impl PartialEq for FocusedInput {
    fn eq(&self, other: &Self) -> bool {
        self.window_element.x == other.window_element.x
            && self.window_element.y == other.window_element.y
            && self.window_element.title == other.window_element.title
            && self.window_element.class_name == other.window_element.class_name
            && self.input_element.x == other.input_element.x
            && self.input_element.y == other.input_element.y
            && self.input_element.text == other.input_element.text
            && self.input_element.control_type == other.input_element.control_type
    }
}

/// 获取当前聚焦输入框及其在窗口内的相对位置
pub fn get_focused_input() -> Option<FocusedInput> {
    unsafe {
        // 1. 获取前台窗口句柄
        let window_element = window::get_current_window()?;
        // 3. 用 UI Automation 获取当前聚焦控件
        let automation = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_ALL).ok()?;
        let focused = automation.GetFocusedElement().ok()?;
        // 4. 获取控件的屏幕坐标
        let control_type = focused.CurrentControlType().ok()?;
        let text = focused.CurrentName().ok()?;
        let text = text.to_string();
        let mut content = String::new();
        if let Ok(pattern_obj) = focused.GetCurrentPattern(UIA_ValuePatternId) {
            if let Ok(value_pattern) = pattern_obj.cast::<IUIAutomationValuePattern>() {
                if let Ok(value) = value_pattern.CurrentValue() {
                    content = value.to_string();
                }
            }
        }
        if !super::app_element::edit_in_app(&window_element.class_name, &text, control_type.0, &content) {
            debug!("[get_focused_input] not edit, current window: {:?}, input_text: {}, control_type: {}, content: {}",
                window_element, text, control_type.0, content);
            return None;
        }
        
        let rect = focused.CurrentBoundingRectangle().ok()?;
        // 7. 构造 UIElement
        let input_element = UIElement {
            text,
            x: rect.left,
            y: rect.top,
            z: 0,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
            window_handle: window_element.window_handle,
            control_type: control_type.0,
            element_type: 0,
            content,
        };
        debug!("[get_focused_input] current window: {:?}, current input: {:?}, ", window_element, input_element);
        Some(FocusedInput { window_element, input_element })
    }
}

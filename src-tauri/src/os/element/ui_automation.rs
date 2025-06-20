use log::{debug, error, info};
use serde::Serialize;
use windows::Win32::{ System::Com::*, UI::Accessibility::*};
use windows::core::Interface;

use crate::os::{window, WindowElement};

#[derive(Clone, Debug, Serialize)]
pub struct UIElement {
    pub id: String,
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub width: i32,
    pub height: i32,
    pub window_id: i64,
    pub control_type: i32,
    // element_type: 0-default, 1-window, 2-pane, 3-tab, 4-button, 5-scrollbar
    pub element_type: usize,
    pub content: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FocusedInput {
    pub window_element: WindowElement,
    pub input_element: UIElement,
}

impl Eq for FocusedInput {}

impl PartialEq for FocusedInput {
    fn eq(&self, other: &Self) -> bool {
        self.window_element.title == other.window_element.title
        && self.window_element.id == other.window_element.id
        && self.window_element.class_name == other.window_element.class_name
        && self.input_element.text == other.input_element.text
        && self.input_element.id == other.input_element.id
        && self.input_element.control_type == other.input_element.control_type
    }
}

/// 获取当前聚焦输入框及其在窗口内的相对位置
pub fn get_focused_input() -> Option<FocusedInput> {
    debug!("[get_focused_input] called");
    unsafe {
        // 1. 获取前台窗口句柄
        let window_element = window::get_current_window()?;
        // 3. 用 UI Automation 获取当前聚焦控件
        let automation = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_ALL).ok()?;
        let focused = automation.GetFocusedElement().ok()?;
        let automation_id = focused.CurrentAutomationId().ok()?;
        let automation_id = automation_id.to_string();
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
        if !super::app_element::is_edit_element_in_app(&window_element.app, &window_element.class_name, &text, control_type.0, &content) {
            debug!("[get_focused_input] not edit, current window: {:?}, input_text: {}, control_type: {}, content: {}",
                window_element, text, control_type.0, content);
            return None;
        }
        
        let rect = focused.CurrentBoundingRectangle().ok()?;
        // 7. 构造 UIElement
        let input_element = UIElement {
            id: automation_id,
            text,
            x: rect.left,
            y: rect.top,
            z: 0,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
            window_id: window_element.id,
            control_type: control_type.0,
            element_type: 0,
            content,
        };
        info!("[get_focused_input] found focused input in app: {}", window_element.app);
        debug!("[get_focused_input] current window: {:?}, current input: {:?}, ", window_element, input_element);
        Some(FocusedInput { window_element, input_element })
    }
}

pub fn fill_input(focused_input: FocusedInput, selected_chars: String) {
    debug!("[ui_automation::fill_input] selected_chars: {}", selected_chars);
    let current_content = focused_input.input_element.content.clone();
    let new_content = current_content + &selected_chars;
    debug!("[ui_automation::fill_input] new_content: {}", new_content);
    unsafe {
        let automation = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_ALL).ok();
        if let Some(automation) = automation {
            if let Ok(element) = automation.GetFocusedElement() {
                if let Ok(pattern_obj) = element.GetCurrentPattern(UIA_ValuePatternId) {
                    if let Ok(value_pattern) = pattern_obj.cast::<IUIAutomationValuePattern>() {
                        let bstr = windows::core::BSTR::from(new_content.clone());
                        match value_pattern.SetValue(&bstr) {
                            Ok(_) => {
                                info!("[ui_automation::fill_input] SetValue success, filled {} chars", selected_chars.len());
                            }
                            Err(e) => {
                                error!("[ui_automation::fill_input] SetValue failed: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }
    
    // TODO 剪贴板粘贴方案
    // unsafe {
    //     // 1. 让目标窗口成为前台窗口
    //     SetForegroundWindow(HWND(focused_input.window_element.id as usize as *mut _));
    //     // 2. 再 SetFocus 到输入框
    //     if let Some(automation) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_ALL).ok() {
    //         if let Ok(element) = automation.GetFocusedElement() {
    //             element.SetFocus().ok();
    //         }
    //     }
    // }
    // // 3. 再 SendInput
    // unsafe {
    //     let mut inputs = [
    //         INPUT {
    //             r#type: INPUT_KEYBOARD,
    //             Anonymous: std::mem::zeroed(),
    //         },
    //         INPUT {
    //             r#type: INPUT_KEYBOARD,
    //             Anonymous: std::mem::zeroed(),
    //         },
    //         INPUT {
    //             r#type: INPUT_KEYBOARD,
    //             Anonymous: std::mem::zeroed(),
    //         },
    //         INPUT {
    //             r#type: INPUT_KEYBOARD,
    //             Anonymous: std::mem::zeroed(),
    //         },
    //     ];
    //     // Ctrl down
    //     inputs[0].Anonymous.ki = KEYBDINPUT {
    //         wVk: VK_CONTROL,
    //         wScan: 0,
    //         dwFlags: KEYBD_EVENT_FLAGS(0),
    //         time: 0,
    //         dwExtraInfo: 0,
    //     };
    //     // V down
    //     inputs[1].Anonymous.ki = KEYBDINPUT {
    //         wVk: VK_V,
    //         wScan: 0,
    //         dwFlags: KEYBD_EVENT_FLAGS(0),
    //         time: 0,
    //         dwExtraInfo: 0,
    //     };
    //     // V up
    //     inputs[2].Anonymous.ki = KEYBDINPUT {
    //         wVk: VK_V,
    //         wScan: 0,
    //         dwFlags: KEYEVENTF_KEYUP,
    //         time: 0,
    //         dwExtraInfo: 0,
    //     };
    //     // Ctrl up
    //     inputs[3].Anonymous.ki = KEYBDINPUT {
    //         wVk: VK_CONTROL,
    //         wScan: 0,
    //         dwFlags: KEYEVENTF_KEYUP,
    //         time: 0,
    //         dwExtraInfo: 0,
    //     };
    //     SendInput(&[inputs[0]], std::mem::size_of::<INPUT>() as i32);
    //     thread::sleep(Duration::from_millis(50));
    //     SendInput(&[inputs[1]], std::mem::size_of::<INPUT>() as i32);
    //     thread::sleep(Duration::from_millis(50));
    //     SendInput(&[inputs[2]], std::mem::size_of::<INPUT>() as i32);
    //     thread::sleep(Duration::from_millis(50));
    //     SendInput(&[inputs[3]], std::mem::size_of::<INPUT>() as i32);
    // }
    // // 4. 粘贴后再关闭 overlay
    // thread::sleep(Duration::from_millis(30));
    // // 恢复原剪贴板
    // let old_clipboard = get_clipboard_text();
    // if let Some(old) = old_clipboard {
    //     set_clipboard_text(&old);
    // }
    // debug!("[ui_automation::fill_input] fallback clipboard paste done");
}
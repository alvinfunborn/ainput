use log::{debug, error, info};
use serde::Serialize;
use windows::Win32::{ System::Com::*, UI::Accessibility::*};
use windows::core::Interface;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MapVirtualKeyW, MAPVK_VK_TO_VSC,
    SendInput, VK_LCONTROL, VK_V,
    VIRTUAL_KEY,
};

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
        debug!("[get_focused_input] found focused input in app: {}, current window: {:?}, current input: {:?}, ", window_element.app, window_element, input_element);
        Some(FocusedInput { window_element, input_element })
    }
}

pub fn fill_input(focused_input: FocusedInput, selected_chars: String) {
    debug!("[ui_automation::fill_input] selected_chars: {}", selected_chars);
    let app = focused_input.window_element.app.clone();
    let ui_cfg = crate::config::get_config().unwrap().ui_automation;
    let is_self_draw = ui_cfg.self_draw_apps.contains(&app);
    if is_self_draw {
        debug!("[ui_automation::fill_input] app {} is in self_draw_apps, use clipboard paste", app);
        // 复制并粘贴选中的候选字符（不尝试读取/设置原有内容，直接在光标处粘贴）
        let text_to_paste = selected_chars;
        let original_clipboard = crate::os::clipboard::windows_clipboard::get_clipboard_text();
        let set_ok = crate::os::clipboard::windows_clipboard::set_clipboard_text(&text_to_paste);
        if !set_ok {
            error!("[ui_automation::fill_input] failed to set clipboard text for paste in app {}", app);
            return;
        }

        unsafe {
            // 使用扫描码发送 Ctrl+V，兼容更多应用
            let sc_ctrl = MapVirtualKeyW(VK_LCONTROL.0 as u32, MAPVK_VK_TO_VSC) as u16;
            let sc_v = MapVirtualKeyW(VK_V.0 as u32, MAPVK_VK_TO_VSC) as u16;

            let mut seq: [INPUT; 4] = [
                INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT::default() } },
                INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT::default() } },
                INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT::default() } },
                INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT::default() } },
            ];
            // Ctrl down (scancode)
            seq[0].Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: sc_ctrl, dwFlags: KEYEVENTF_SCANCODE, time: 0, dwExtraInfo: 0 };
            // V down (scancode)
            seq[1].Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: sc_v, dwFlags: KEYEVENTF_SCANCODE, time: 0, dwExtraInfo: 0 };
            // V up (scancode)
            seq[2].Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: sc_v, dwFlags: KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 };
            // Ctrl up (scancode)
            seq[3].Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: sc_ctrl, dwFlags: KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 };

            let sent = SendInput(&seq, std::mem::size_of::<INPUT>() as i32);
            debug!("[ui_automation::fill_input] SendInput Ctrl+V (scancode) sent events: {}", sent);
        }

        // 等待系统处理粘贴
        std::thread::sleep(std::time::Duration::from_millis(160));
        if let Some(orig) = original_clipboard.as_ref() {
            let _ = crate::os::clipboard::windows_clipboard::set_clipboard_text(orig);
        }
        return;
    } else {
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
    }
}
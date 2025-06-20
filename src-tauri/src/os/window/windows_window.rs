use log::{error};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextW, IsWindowVisible
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
use windows::Win32::Foundation::CloseHandle;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;

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

            let mut process_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id as *mut u32));
            let process_handle = OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                false,
                process_id,
            );
            let mut exe_name = String::from("");
            if let Ok(process_handle) = process_handle {
                let mut exe_path = [0u16; 512];
                let len = K32GetModuleFileNameExW(
                    Some(process_handle),
                    None,
                    &mut exe_path,
                ) as usize;
                if len > 0 {
                    let os_string = OsString::from_wide(&exe_path[..len]);
                    let path = Path::new(&os_string);
                    if let Some(file_name) = path.file_name() {
                        exe_name = file_name.to_string_lossy().to_string();
                    } else {
                        exe_name = os_string.to_string_lossy().to_string();
                    }
                }
                let _ = CloseHandle(process_handle);
            }

            let window_element = WindowElement {
                id: hwnd.0 as i64,
                app: exe_name,
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
                title: title.clone(),
                class_name: class_name.clone(),
            };
            Some(window_element)
        } else {
            None
        }
    }
}

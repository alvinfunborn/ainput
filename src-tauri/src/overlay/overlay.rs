use log::{debug, info};
use tauri::{Emitter, Manager};

use crate::{config, APP_HANDLE};

// 直接获取 main 窗口
fn get_main_window() -> Option<tauri::WebviewWindow> {
    APP_HANDLE.lock().unwrap().as_ref().and_then(|handle| handle.get_webview_window("main"))
}

pub fn update_overlay(candidate: String) {
    debug!("[update_overlay] updating overlay with candidate of length: {}", candidate.len());
    if let Some(window) = get_main_window() {
        let _ = window.emit("update_overlay", candidate);
    }
}

pub fn hide_overlay() {
    info!("[hide_overlay] hiding overlay window");
    if let Some(window) = get_main_window() {
        let _ = window.emit("hide_overlay", ());
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize { width: 0.0, height: 0.0 }));
        let _ = window.set_position(tauri::LogicalPosition::new(0, 0));
    }
}

pub fn select_candidate(num: i32) {
    info!("[select_candidate] emitting select_candidate event with num: {}", num);
    if let Some(window) = get_main_window() {
        let _ = window.emit("select_candidate", num);
    }
}

#[tauri::command]
pub fn resize_overlay_window(width: f64, height: f64) {
    info!("[resize_overlay_window] resizing to width: {}, height: {}", width, height);
    if let Some(window) = get_main_window() {
        if let Ok(position) = window.outer_position() {
            // 获取当前屏幕信息
            if let Ok(Some(monitor)) = window.current_monitor() {
                let scale = monitor.scale_factor();
                let monitor_x = monitor.position().x as f64;
                let monitor_width = monitor.size().width as f64 / scale;
                let screen_left = monitor_x;
                let screen_right = monitor_x + monitor_width;
                // 将物理坐标转为逻辑坐标
                let x: f64 = position.x as f64 / scale;
                let y: f64 = position.y as f64 / scale;
                debug!("[resize_overlay_window] x: {:?}, y: {:?}, screen_left: {:?}, screen_right: {:?}", x, y, screen_left, screen_right);
                let mut new_x = x;
                if x + width > screen_right {
                    new_x = screen_right - width;
                    if new_x < screen_left { new_x = screen_left; }
                    debug!("[resize_overlay_window] overlay exceeds right edge, move left to x={}", new_x);
                }
                let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
                let _ = window.set_position(tauri::LogicalPosition::new(new_x, y));
            } else {
                let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
            }
        } else {
            let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
        }
        // top_window(&window);
    }
}

pub fn top_window(window: &tauri::WebviewWindow) {
    info!("[top_window] setting window to be always-on-top (NOACTIVATE)");
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = window.hwnd() {
            let hwnd_raw = hwnd.0;
            unsafe {
                use windows::Win32::Foundation::HWND;
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetWindowLongW, SetWindowLongW, SetWindowPos,
                    GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_NOACTIVATE,
                    HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW
                };

                let style = GetWindowLongW(HWND(hwnd_raw as *mut _), GWL_EXSTYLE);
                SetWindowLongW(
                    HWND(hwnd_raw as *mut _),
                    GWL_EXSTYLE,
                    style | WS_EX_NOACTIVATE.0 as i32 | WS_EX_LAYERED.0 as i32,
                );
                SetWindowPos(
                    HWND(hwnd_raw as *mut _),
                    Some(HWND_TOPMOST),
                    0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                );
            }
        }
    }
}

#[tauri::command]
pub fn get_overlay_style() -> String {
    info!("[get_overlay_style] providing overlay style to frontend");
    let config = config::get_config().unwrap();
    config.overlay.style
}
use log::{debug, info};
use tauri::{Emitter, Manager};

use crate::{config, APP_HANDLE};

// 直接获取 main 窗口
fn get_main_window() -> Option<tauri::WebviewWindow> {
    APP_HANDLE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|handle| handle.get_webview_window("main"))
}

pub fn update_overlay(candidate: String) {
    debug!(
        "[update_overlay] updating overlay with candidate of length: {}",
        candidate.len()
    );
    if let Some(window) = get_main_window() {
        let _ = window.emit("update_overlay", candidate);
    }
}

#[cfg(target_os = "macos")]
pub fn replace_overlay(candidate: String) {
    if let Some(window) = get_main_window() {
        let _ = window.emit("replace_overlay", candidate);
    }
}

#[cfg(target_os = "macos")]
pub fn show_insertion_error() {
    if let Some(window) = get_main_window() {
        let _ = window.emit(
            "insertion_error",
            "未确认写入，请检查输入框；继续输入可重新获取补全",
        );
    }
}

pub fn show_request_error() {
    if let Some(window) = get_main_window() {
        let _ = window.emit("request_error", "暂时无法获取补全，请继续输入后重试");
    }
}

pub fn hide_overlay() {
    debug!("[hide_overlay] hiding overlay window");
    if let Some(window) = get_main_window() {
        let _ = window.emit("hide_overlay", ());
        #[cfg(target_os = "windows")]
        {
            let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: 0.0,
                height: 0.0,
            }));
            let _ = window.hide();
        }
        #[cfg(target_os = "macos")]
        {
            let clone = window.clone();
            let _ = window.run_on_main_thread(move || {
                if let Ok(native) = clone.ns_window() {
                    unsafe {
                        crate::macos_window::set_overlay_visible(native.cast(), false);
                    }
                }
            });
        }
    }
}

#[cfg(target_os = "windows")]
pub fn select_candidate(num: i32) {
    info!(
        "[select_candidate] emitting select_candidate event with num: {}",
        num
    );
    if let Some(window) = get_main_window() {
        let _ = window.emit("select_candidate", num);
    }
}

#[tauri::command]
pub fn resize_overlay_window(width: f64, height: f64) {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(window) = get_main_window() {
            let clone = window.clone();
            let _ = window.run_on_main_thread(move || {
                if !crate::input::get_input_state() {
                    return;
                }
                if let Ok(native) = clone.ns_window() {
                    let (x, y) = *MAC_ANCHOR.lock().unwrap();
                    unsafe {
                        let _ =
                            clone.emit("overlay_limits", crate::macos_window::overlay_limits(x, y));
                        crate::macos_window::resize_and_position(
                            native.cast(),
                            x,
                            y,
                            width,
                            height,
                        );
                    }
                }
            });
        }
        return;
    }
    #[cfg(target_os = "windows")]
    {
        info!(
            "[resize_overlay_window] resizing to width: {}, height: {}",
            width, height
        );
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
                        if new_x < screen_left {
                            new_x = screen_left;
                        }
                        debug!(
                            "[resize_overlay_window] overlay exceeds right edge, move left to x={}",
                            new_x
                        );
                    }
                    let _ =
                        window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
                    let _ = window.set_position(tauri::LogicalPosition::new(new_x, y));
                } else {
                    let _ =
                        window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
                }
            } else {
                let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
            }
            top_window(&window);
        }
    }
}

pub fn top_window(window: &tauri::WebviewWindow) {
    info!("[top_window] setting window to be always-on-top (NOACTIVATE)");
    #[cfg(target_os = "macos")]
    {
        let clone = window.clone();
        let _ = window.run_on_main_thread(move || {
            if let Ok(native) = clone.ns_window() {
                unsafe {
                    crate::macos_window::configure_overlay(native.cast());
                }
            }
        });
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = window.hwnd() {
            let hwnd_raw = hwnd.0;
            unsafe {
                use windows::Win32::Foundation::HWND;
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
                    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
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
                    0,
                    0,
                    0,
                    0,
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
#[cfg(target_os = "macos")]
static MAC_ANCHOR: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0));

pub fn position_overlay(x: i32, y: i32) {
    if get_main_window().is_none() {
        return;
    }
    let cfg = config::get_config().unwrap().overlay;
    #[cfg(target_os = "macos")]
    {
        // Accessibility and CGDisplay share global logical points, even on
        // Retina displays. Do not divide these coordinates by a scale factor.
        let anchor = (f64::from(x + cfg.relative_x), f64::from(y + cfg.relative_y));
        *MAC_ANCHOR.lock().unwrap() = anchor;
        if let Some(window) = get_main_window() {
            let clone = window.clone();
            let _ = window.run_on_main_thread(move || {
                let limits = unsafe { crate::macos_window::overlay_limits(anchor.0, anchor.1) };
                let _ = clone.emit("overlay_limits", limits);
            });
        }
    }
    #[cfg(target_os = "windows")]
    if let Some(window) = get_main_window() {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        let scale = window
            .current_monitor()
            .ok()
            .flatten()
            .map(|m| m.scale_factor())
            .unwrap_or(1.0);
        let _ = window.set_position(tauri::PhysicalPosition::new(
            x + (cfg.relative_x as f64 * scale) as i32,
            y + (cfg.relative_y as f64 * scale) as i32,
        ));
        top_window(&window);
    }
}

pub mod hook;
pub mod keyboard;

use std::sync::Mutex;
use log::{debug, error, info};
use once_cell::sync::Lazy;
use crate::{ai::ai_client, config, context, os::element, overlay::{self, overlay::resize_overlay_window}};
use tauri::{LogicalPosition, Manager};
use tauri_plugin_notification::NotificationExt;
use crate::APP_HANDLE;
use std::sync::RwLock;
use std::thread;
use tauri::async_runtime::JoinHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering, AtomicU64};

static INPUT_STATE: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
static FORMER_FOCUSED_INPUT: Lazy<RwLock<Option<element::FocusedInput>>> = Lazy::new(|| RwLock::new(None));
static SELECTED_CANDIDATE: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new(String::new()));
static CANDIDATE: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new(String::new()));
static OVERLAY_TASK_HANDLE: Lazy<Mutex<Option<JoinHandle<()>>>> = Lazy::new(|| Mutex::new(None));
static OVERLAY_CANCEL_TOKEN: Lazy<Mutex<Option<Arc<AtomicBool>>>> = Lazy::new(|| Mutex::new(None));
static TASK_GENERATION: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

pub fn set_input_state(state: bool) {
    info!("[set_input_state] set to {}", state);
    let mut input_state = INPUT_STATE.lock().unwrap();
    *input_state = state;
}

pub fn get_input_state() -> bool {
    let input_state = INPUT_STATE.lock().unwrap();
    *input_state
}

fn start_overlay(focused_input: element::FocusedInput) {
    info!("[start_overlay] for app: {}, input: {}", focused_input.window_element.app, focused_input.input_element.text);
    set_input_state(true);
    end_overlay_stream_task();

    let my_generation = TASK_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;

    overlay::overlay::hide_overlay();
    resize_overlay_window(200.0, 40.0);
    *CANDIDATE.write().unwrap() = String::new();
    *SELECTED_CANDIDATE.write().unwrap() = String::new();
    let cancel_token = Arc::new(AtomicBool::new(false));
    {
        let mut token_guard = OVERLAY_CANCEL_TOKEN.lock().unwrap();
        *token_guard = Some(cancel_token.clone());
    }
    if let Some(window) = APP_HANDLE.lock().unwrap().as_ref().and_then(|h| h.get_webview_window("main")) {
        let _ = window.set_position(LogicalPosition::new(focused_input.input_element.x as f64, focused_input.input_element.y as f64));
        if let Some(monitor) = window.current_monitor().unwrap() {
            let scale_factor = monitor.scale_factor();
            let overlay_config = config::get_config().unwrap().system;
            let x = (focused_input.input_element.x as f64 + overlay_config.overlay_relative_x as f64 * scale_factor) / scale_factor;
            let y = (focused_input.input_element.y as f64 + overlay_config.overlay_relative_y as f64 * scale_factor) / scale_factor;
            debug!("[start_overlay] x: {}, y: {}", x, y);
            let _ = window.set_position(LogicalPosition::new(x, y));
            overlay::overlay::top_window(&window);
        }
    }
    
    let handle = tauri::async_runtime::spawn(async move {
        let current_generation = TASK_GENERATION.load(Ordering::Relaxed);
        if my_generation < current_generation {
            info!("[start_overlay] Task generation {} is obsolete, current is {}. Aborting.", my_generation, current_generation);
            return;
        }

        let context = match tokio::task::spawn_blocking(move || context::Context::new(&focused_input)).await {
            Ok(Some(ctx)) => ctx,
            Ok(None) => {
                error!("[start_overlay] Context::new returned None.");
                return;
            }
            Err(_) => {
                error!("[start_overlay] Failed to create context in background task (JoinError).");
                return;
            }
        };

        let current_generation = TASK_GENERATION.load(Ordering::Relaxed);
        if my_generation < current_generation {
            info!("[start_overlay] Task generation {} became obsolete during context gathering, current is {}. Aborting.", my_generation, current_generation);
            return;
        }

        let client = ai_client::AiClient::new();
        let cancel_token_clone = cancel_token.clone();
        let api_key = config::get_config().unwrap().ai_client.api_key;
        let result = match api_key.is_empty() {
            true => client.stream_request_mock(context, |c| {
                debug!("[start_overlay] stream_request_mock: {}", c);
                let mut candidate = CANDIDATE.write().unwrap();
                candidate.push_str(&c);
                overlay::overlay::update_overlay(c);
            }, cancel_token_clone).await,
            false => client.stream_request_ai(context, |c| {
                debug!("[start_overlay] stream_request_ai: {}", c);
                let mut candidate = CANDIDATE.write().unwrap();
                candidate.push_str(&c);
                overlay::overlay::update_overlay(c);
            }, cancel_token_clone).await,
        };

        if let Err(e) = result {
            error!("[start_overlay] stream_request_ai failed: {}", e);
            if let Some(app_handle) = APP_HANDLE.lock().unwrap().as_ref() {
                let _ = app_handle.notification()
                    .builder()
                    .title("ainput Network Error")
                    .body(format!("Failed to get completion: {}", e))
                    .show();
            }
        }
    });
    *OVERLAY_TASK_HANDLE.lock().unwrap() = Some(handle);
}

fn end_overlay_stream_task() {
    if let Some(handle) = OVERLAY_TASK_HANDLE.lock().unwrap().take() {
        debug!("[end_overlay_stream_task] abort handle");
        handle.abort();
    }
    if let Some(token) = OVERLAY_CANCEL_TOKEN.lock().unwrap().as_ref() {
        debug!("[end_overlay_stream_task] abort token");
        token.store(true, Ordering::SeqCst);
    }
}

fn end_overlay() {
    info!("[end_overlay]");
    set_input_state(false);
    overlay::overlay::hide_overlay();
    end_overlay_stream_task();
}

fn select_candidate(num: i32) {
    info!("[select_candidate] selecting {} chars", num);
    let (selected_chars, selected_num): (String, i32) = {
        let mut candidate = CANDIDATE.write().unwrap();
        let mut selected_num = num;
        let candidate_chars: Vec<char> = candidate.chars().collect();
        if selected_num == -1 {
            selected_num = candidate_chars.len() as i32;
        }
        if selected_num > candidate_chars.len() as i32 {
            selected_num = candidate_chars.len() as i32;
        }
        let selected_chars: String = candidate_chars[..selected_num as usize].iter().collect();
        if selected_chars.is_empty() {
            debug!("[select_candidate] selected_chars is empty");
            return;
        }
        let rest: String = candidate_chars[selected_num as usize..].iter().collect();
        *candidate = rest;
        (selected_chars, selected_num)
    };
    debug!("[select_candidate] call element::fill_input");
    let mut current_selected_candidate = SELECTED_CANDIDATE.write().unwrap();
    *current_selected_candidate = format!("{}{}", current_selected_candidate, selected_chars);
    element::fill_input(selected_chars);
    debug!("[select_candidate] call overlay::overlay::select_candidate");
    overlay::overlay::select_candidate(selected_num);
}

fn save_history(focused_input: &element::FocusedInput) {
    info!("[save_history] for app: {}", focused_input.window_element.app);
    let mut conn = context::history::db::establish_connection();
    let input_history = context::history::history::InputHistory {
        window_id: focused_input.window_element.id,
        window_app: focused_input.window_element.app.clone(),
        window_title: focused_input.window_element.title.clone(),
        window_class_name: focused_input.window_element.class_name.clone(),
        window_x: focused_input.window_element.x,
        window_y: focused_input.window_element.y,
        window_width: focused_input.window_element.width,
        window_height: focused_input.window_element.height,
        input_id: focused_input.input_element.id.clone(),
        input_title: focused_input.input_element.text.clone(),
        input_control_type: focused_input.input_element.control_type,
        input_x: focused_input.input_element.x,
        input_y: focused_input.input_element.y,
        input_width: focused_input.input_element.width,
        input_height: focused_input.input_element.height,
        input_content: focused_input.input_element.content.clone(),
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64,
    };
    context::history::history::save_history(&mut conn, &input_history);
}

pub fn listen_input_state() {
    info!("[listen_input_state] starting input state listener thread");
    let focus_interval = config::get_config().unwrap().system.refresh_overlay_interval;
    thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(focus_interval));
            match element::get_current_focus_info() {
                Some(focused_input) => {
                    let mut guard = FORMER_FOCUSED_INPUT.write().unwrap();
                    if let Some(former_focused_input) = guard.as_ref() {
                        if !former_focused_input.eq(&focused_input) {
                            info!("[listen_input_state] focus changed");
                            save_history(&former_focused_input);
                            *guard = Some(focused_input.clone());
                            start_overlay(focused_input);
                        } else {
                            let new_content = &focused_input.input_element.content;
                            let old_content = &former_focused_input.input_element.content;
                            if new_content.ends_with("\u{200b}") 
                                || new_content.ends_with("\u{200c}") 
                                || new_content.ends_with("\u{200d}") 
                                || new_content.ends_with("\u{FEFF}") 
                            {
                                *guard = Some(focused_input.clone());
                            } else if new_content != old_content {
                                let restart_overlay = {
                                    let candidate = CANDIDATE.read().unwrap();
                                    if !candidate.is_empty() {
                                        let selected_candidate = SELECTED_CANDIDATE.read().unwrap();
                                        let full_candidate = format!("{}{}", selected_candidate, candidate);
                                        let candidate_chars: Vec<char> = full_candidate.chars().collect();
                                        let new_content_chars: Vec<char> = new_content.chars().collect();
                                        let mut matched = false;
                                        for i in 0..candidate_chars.len() {
                                            if new_content_chars.len() <= i {
                                                break;
                                            }
                                            let candidate_prefix: String = candidate_chars[..=i].iter().collect();
                                            let new_content_suffix: String = new_content_chars[new_content_chars.len()-i-1..].iter().collect();
                                            if new_content_suffix == candidate_prefix {
                                                matched = true;
                                                break;
                                            }
                                        }
                                        !matched
                                    } else {
                                        true
                                    }
                                };
                                debug!("[listen_input_state] restart_overlay: {}", restart_overlay);
                                if restart_overlay {
                                    info!("[listen_input_state] content changed, restarting overlay");
                                    save_history(&former_focused_input);
                                    *guard = Some(focused_input.clone());
                                    start_overlay(focused_input);
                                } else {
                                    *guard = Some(focused_input.clone());
                                }
                            } else {
                                *guard = Some(focused_input.clone());
                            }
                        }
                    } else {
                        info!("[listen_input_state] new input focused");
                        *guard = Some(focused_input.clone());
                        start_overlay(focused_input);
                    }
                }
                None => {
                    let mut guard = FORMER_FOCUSED_INPUT.write().unwrap();
                    if let Some(former_focused_input) = &*guard {
                        info!("[listen_input_state] focus lost");
                        save_history(former_focused_input);
                    }
                    *guard = None;
                    debug!("[listen_input_state] focused_input: None");
                    end_overlay();
                }
            }
        }
    });
}




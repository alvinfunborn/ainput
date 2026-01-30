pub mod hook;
pub mod keyboard;

use std::sync::Mutex;
use std::panic;
use log::{debug, error, info};
use once_cell::sync::Lazy;
use crate::{ai::ai_client, config, context, os::element, overlay::{self, overlay::resize_overlay_window}};
use tauri::{LogicalPosition, Manager};
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
static OVERLAY_IS_RUNNING: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));
static LAST_OVERLAY_START_TIME: Lazy<Mutex<std::time::Instant>> = Lazy::new(|| Mutex::new(std::time::Instant::now()));
/// 上次 AI 请求是否返回了空结果（用于决定是否需要防抖）
static LAST_AI_WAS_EMPTY: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

pub fn set_input_state(state: bool) {
    debug!("[set_input_state] set to {}", state);
    let mut input_state = INPUT_STATE.lock().unwrap();
    *input_state = state;
}

pub fn get_input_state() -> bool {
    let input_state = INPUT_STATE.lock().unwrap();
    *input_state
}

pub fn has_candidate() -> bool {
    !CANDIDATE.read().unwrap().is_empty()
}

fn start_overlay(focused_input: element::FocusedInput) {
    info!("[start_overlay] for app: {}, input: {}", focused_input.window_element.app, focused_input.input_element.text);
    set_input_state(true);
    end_overlay_stream_task();

    // 更新最后一次启动时间，用于防抖
    *LAST_OVERLAY_START_TIME.lock().unwrap() = std::time::Instant::now();

    let my_generation = TASK_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;

    overlay::overlay::hide_overlay();
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
            let overlay_config = config::get_config().unwrap().overlay;
            let x = (focused_input.input_element.x as f64 + overlay_config.relative_x as f64 * scale_factor) / scale_factor;
            let y = (focused_input.input_element.y as f64 + overlay_config.relative_y as f64 * scale_factor) / scale_factor;
            debug!("[start_overlay] x: {}, y: {}", x, y);
            let _ = window.set_position(LogicalPosition::new(x, y));
            overlay::overlay::top_window(&window);
        }
    }

    OVERLAY_IS_RUNNING.store(true, Ordering::SeqCst);
    let handle = tauri::async_runtime::spawn(async move {
        let current_generation = TASK_GENERATION.load(Ordering::SeqCst);
        if my_generation < current_generation {
            info!("[start_overlay] Task generation {} is obsolete, current is {}. Aborting.", my_generation, current_generation);
            // Mark not running if still current
            if my_generation == current_generation {
                OVERLAY_IS_RUNNING.store(false, Ordering::SeqCst);
            }
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

        let current_generation = TASK_GENERATION.load(Ordering::SeqCst);
        if my_generation < current_generation {
            info!("[start_overlay] Task generation {} became obsolete during context gathering, current is {}. Aborting.", my_generation, current_generation);
            return;
        }

        let client = ai_client::AiClient::new();
        let cancel_token_clone = cancel_token.clone();
        let received_any_token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let received_any_token_clone = received_any_token.clone();
        let result = client.stream_request_ai(context, move |c| {
                debug!("[start_overlay] stream_request_ai: {}", c);
                received_any_token_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                // 收到内容，标记上次请求非空
                LAST_AI_WAS_EMPTY.store(false, Ordering::SeqCst);
                let mut candidate = CANDIDATE.write().unwrap();
                if candidate.is_empty() && !c.trim().is_empty() {
                    resize_overlay_window(200.0, 40.0);
                }
                candidate.push_str(&c);
                overlay::overlay::update_overlay(c);
            }, cancel_token_clone).await;

        let was_cancelled = cancel_token.load(Ordering::SeqCst);
        let got_tokens = received_any_token.load(std::sync::atomic::Ordering::SeqCst);

        match &result {
            Ok(_) if !got_tokens && !was_cancelled => {
                // AI returned success but no content, mark as empty response
                info!("[start_overlay] AI returned no tokens (empty response)");
                LAST_AI_WAS_EMPTY.store(true, Ordering::SeqCst);
                // Show empty response message, clear first to avoid accumulation
                let error_msg = "⚠ AI returned empty response".to_string();
                {
                    let mut candidate = CANDIDATE.write().unwrap();
                    candidate.clear();
                    resize_overlay_window(300.0, 40.0);
                    candidate.push_str(&error_msg);
                }
                overlay::overlay::update_overlay(error_msg);
            }
            Err(e) => {
                error!("[start_overlay] stream_request_ai failed: {}", e);
                LAST_AI_WAS_EMPTY.store(true, Ordering::SeqCst);
                // Display error message as completion so user knows why it failed
                // Use clear + push to avoid accumulating errors on retries
                let error_msg = format!("⚠ {}", e);
                {
                    let mut candidate = CANDIDATE.write().unwrap();
                    candidate.clear();
                    resize_overlay_window(400.0, 40.0);
                    candidate.push_str(&error_msg);
                }
                overlay::overlay::update_overlay(error_msg);
            }
            _ => {
                // 正常返回内容，或被取消
            }
        }
        // Mark task finished if still the latest generation
        let current_generation = TASK_GENERATION.load(Ordering::SeqCst);
        if my_generation == current_generation {
            OVERLAY_IS_RUNNING.store(false, Ordering::SeqCst);
            // Clear handle and cancel token to reflect finished state
            let _ = OVERLAY_TASK_HANDLE.lock().unwrap().take();
            let _ = OVERLAY_CANCEL_TOKEN.lock().unwrap().take();
        }
    });
    *OVERLAY_TASK_HANDLE.lock().unwrap() = Some(handle);
}

fn end_overlay_stream_task() {
    // Always lock OVERLAY_CANCEL_TOKEN before OVERLAY_TASK_HANDLE to avoid deadlocks
    if let Some(token) = OVERLAY_CANCEL_TOKEN.lock().unwrap().as_ref() {
        debug!("[end_overlay_stream_task] abort token");
        token.store(true, Ordering::SeqCst);
    }
    if let Some(handle) = OVERLAY_TASK_HANDLE.lock().unwrap().take() {
        debug!("[end_overlay_stream_task] abort handle");
        handle.abort();
    }
    OVERLAY_IS_RUNNING.store(false, Ordering::SeqCst);
}

fn end_overlay() {
    set_input_state(false);
    overlay::overlay::hide_overlay();
    end_overlay_stream_task();
}

fn select_candidate(num: i32) {
    info!("[select_candidate] selecting {} chars", num);
    // 读取当前候选的快照，不立即修改全局候选，避免在填充之前将候选清空导致监听线程误判
    let (selected_chars, selected_num, rest_after_select): (String, i32, String) = {
        let current_candidate = CANDIDATE.read().unwrap().clone();
        let mut selected_num = num;
        let candidate_chars: Vec<char> = current_candidate.chars().collect();
        if selected_num == -1 {
            selected_num = candidate_chars.len() as i32;
        }
        if selected_num > candidate_chars.len() as i32 {
            selected_num = candidate_chars.len() as i32;
        }
        let selected_chars: String = candidate_chars[..selected_num as usize].iter().collect();
        let cleaned = selected_chars.trim_end_matches(|c| c == '\r' || c == '\n');
        if cleaned.is_empty() {
            debug!("[select_candidate] selected_chars is empty");
            return;
        }
        let cleaned = cleaned.to_string();
        let cleaned_len = cleaned.chars().count() as i32;
        let rest: String = candidate_chars[selected_num as usize..].iter().collect();
        (cleaned, cleaned_len, rest)
    };

    // 先填充到目标输入框，再更新内部状态与前端 overlay，避免“候选先变空再填充”的竞态
    debug!("[select_candidate] call element::fill_input");
    element::fill_input(selected_chars.clone());

    {
        let mut current_selected_candidate = SELECTED_CANDIDATE.write().unwrap();
        *current_selected_candidate = format!("{}{}", current_selected_candidate, selected_chars);
    }
    {
        let mut candidate = CANDIDATE.write().unwrap();
        *candidate = rest_after_select;
    }

    debug!("[select_candidate] call overlay::overlay::select_candidate");
    overlay::overlay::select_candidate(selected_num);
}

fn save_history(focused_input: &element::FocusedInput) {
    info!("[save_history] for app: {}", focused_input.window_element.app);
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
    if let Err(e) = crate::db::conn::with_connection(|conn| {
        context::history::history::save_history(conn, &input_history);
    }) {
        error!("[save_history] database error: {}", e);
    }
}

pub fn listen_input_state() {
    info!("[listen_input_state] starting input state listener thread");
    let focus_interval = config::get_config().unwrap().overlay.refresh_interval;
    thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(focus_interval));
            match element::get_current_focus_info() {
                Some(focused_input) => {
                    // Decide actions while holding the lock, but perform heavy work after releasing it
                    let (should_start_overlay, save_former_opt) = {
                        let mut guard = FORMER_FOCUSED_INPUT.write().unwrap();
                        if let Some(former_focused_input) = guard.as_ref() {
                            if !former_focused_input.eq(&focused_input) {
                                debug!("[listen_input_state] focus changed");
                                let former_clone = former_focused_input.clone();
                                *guard = Some(focused_input.clone());
                                (true, Some(former_clone))
                            } else {
                                let new_content = &focused_input.input_element.content;
                                let old_content = &former_focused_input.input_element.content;
                                if new_content.ends_with("\u{200b}")
                                    || new_content.ends_with("\u{200c}")
                                    || new_content.ends_with("\u{200d}")
                                    || new_content.ends_with("\u{FEFF}")
                                {
                                    *guard = Some(focused_input.clone());
                                    (false, None)
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
                                                let new_content_suffix: String = new_content_chars[new_content_chars.len() - i - 1..].iter().collect();
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
                                        let former_clone = former_focused_input.clone();
                                        *guard = Some(focused_input.clone());
                                        (true, Some(former_clone))
                                    } else {
                                        info!("[listen_input_state] content changed; no need to restart overlay");
                                        *guard = Some(focused_input.clone());
                                        (false, None)
                                    }
                                } else {
                                    // 内容不变，可能是自绘制控件导致无法感知变化。
                                    // 若当前处于输入态，且没有候选、也没有进行中的候选请求，则主动重启 overlay。
                                    // 但需要防止 AI 返回空结果时无限重试，仅在上次为空响应时增加防抖机制。
                                    let should_force_restart = {
                                        let in_input = get_input_state();
                                        let has_candidate = !CANDIDATE.read().unwrap().is_empty();
                                        let has_running_task = OVERLAY_IS_RUNNING.load(Ordering::SeqCst);
                                        let last_was_empty = LAST_AI_WAS_EMPTY.load(Ordering::SeqCst);
                                        info!("[listen_input_state] in_input: {}, has_candidate: {}, has_running_task: {}, last_was_empty: {}",
                                              in_input, has_candidate, has_running_task, last_was_empty);
                                        if in_input && !has_candidate && !has_running_task {
                                            if last_was_empty {
                                                // 上次 AI 返回空，需要防抖避免无限循环
                                                let last_start = LAST_OVERLAY_START_TIME.lock().unwrap();
                                                let elapsed = last_start.elapsed();
                                                let retry_interval_ms = config::get_config().map(|c| c.overlay.retry_interval).unwrap_or(3000);
                                                let min_interval = std::time::Duration::from_millis(retry_interval_ms);
                                                if elapsed >= min_interval {
                                                    info!("[listen_input_state] last AI was empty, but retry interval passed, allowing restart");
                                                    true
                                                } else {
                                                    debug!("[listen_input_state] debounce: last AI was empty, only {}ms since last start, need {}ms",
                                                           elapsed.as_millis(), min_interval.as_millis());
                                                    false
                                                }
                                            } else {
                                                // 上次 AI 正常返回，自绘制控件可能有新输入，立即重启
                                                true
                                            }
                                        } else {
                                            false
                                        }
                                    };
                                    *guard = Some(focused_input.clone());
                                    if should_force_restart {
                                        info!("[listen_input_state] content unchanged; in input without candidates or running task, restarting overlay");
                                        (true, None)
                                    } else {
                                        info!("[listen_input_state] content unchanged; no need to restart overlay");
                                        (false, None)
                                    }
                                }
                            }
                        } else {
                            info!("[listen_input_state] new input focused");
                            *guard = Some(focused_input.clone());
                            (true, None)
                        }
                    };

                    if let Some(former) = save_former_opt.as_ref() {
                        let former_clone = former.clone();
                        if let Err(e) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                            save_history(&former_clone);
                        })) {
                            error!("[listen_input_state] save_history panicked: {:?}", e);
                        }
                    }
                    if should_start_overlay {
                        start_overlay(focused_input);
                    }
                }
                None => {
                    // Clone former focused input, clear state, then perform heavy work outside the lock
                    let former_opt = {
                        let mut guard = FORMER_FOCUSED_INPUT.write().unwrap();
                        let former = guard.clone();
                        *guard = None;
                        former
                    };
                    if let Some(former_focused_input) = former_opt {
                        info!("[listen_input_state] focus lost");
                        // Wrap save_history in catch_unwind to ensure end_overlay() always runs
                        if let Err(e) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                            save_history(&former_focused_input);
                        })) {
                            error!("[listen_input_state] save_history panicked on focus lost: {:?}", e);
                        }
                    }
                    debug!("[listen_input_state] focused_input: None");
                    end_overlay();
                }
            }
        }
    });
}




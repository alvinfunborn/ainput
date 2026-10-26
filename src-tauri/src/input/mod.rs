#[cfg(target_os = "windows")]
pub mod hook;
#[cfg(target_os = "macos")]
#[path = "hook_macos.rs"]
pub mod hook;
#[cfg(target_os = "windows")]
pub mod keyboard;

use crate::{
    ai::ai_client,
    config, context,
    os::element,
    overlay::{self, overlay::resize_overlay_window},
};
use log::{debug, error, info};
use once_cell::sync::Lazy;
use std::panic;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread;
use tauri::async_runtime::JoinHandle;

static INPUT_STATE: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
static FORMER_FOCUSED_INPUT: Lazy<RwLock<Option<element::FocusedInput>>> =
    Lazy::new(|| RwLock::new(None));
static SELECTED_CANDIDATE: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new(String::new()));
static CANDIDATE: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new(String::new()));
static OVERLAY_TASK_HANDLE: Lazy<Mutex<Option<JoinHandle<()>>>> = Lazy::new(|| Mutex::new(None));
static OVERLAY_CANCEL_TOKEN: Lazy<Mutex<Option<Arc<AtomicBool>>>> = Lazy::new(|| Mutex::new(None));
static TASK_GENERATION: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
static OVERLAY_IS_RUNNING: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));
/// Empty/failed requests remain settled until the input or focus changes.
static LAST_AI_WAS_EMPTY: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));
static COMPLETION_ENABLED: AtomicBool = AtomicBool::new(true);
static COMPLETION_LIFECYCLE: Mutex<()> = Mutex::new(());

pub fn completion_enabled() -> bool {
    COMPLETION_ENABLED.load(Ordering::SeqCst)
}

pub fn sync_completion_enabled() {
    let _lifecycle = COMPLETION_LIFECYCLE.lock().unwrap();
    let enabled = config::get_config().map(|c| c.system.completion_enabled).unwrap_or(true);
    if COMPLETION_ENABLED.swap(enabled, Ordering::SeqCst) != enabled {
        end_overlay();
        *FORMER_FOCUSED_INPUT.write().unwrap() = None;
        info!("[completion] enabled={enabled}");
    }
}

pub fn set_input_state(state: bool) {
    debug!("[set_input_state] set to {}", state);
    let mut input_state = INPUT_STATE.lock().unwrap();
    *input_state = state;
}

pub fn get_input_state() -> bool {
    let input_state = INPUT_STATE.lock().unwrap();
    completion_enabled() && *input_state
}

#[cfg(target_os = "windows")]
pub fn has_candidate() -> bool {
    !CANDIDATE.read().unwrap().trim().is_empty()
}

#[cfg(target_os = "macos")]
static KEYBOARD_REVISION: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "macos")]
static ACTIVITY_REASON: AtomicU8 = AtomicU8::new(0);
#[cfg(target_os = "macos")]
static CANDIDATE_REVISION: AtomicU64 = AtomicU64::new(0);
// Unlike TASK_GENERATION, this stays stable when partial acceptance stops the
// stream. A held key may only repeat within the completion it started on.
#[cfg(target_os = "macos")]
static CANDIDATE_SESSION: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "macos")]
static SELECTING: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static INSERTION_FAILED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static CANDIDATE_TARGET: Lazy<RwLock<Option<element::FocusedInput>>> =
    Lazy::new(|| RwLock::new(None));

#[cfg(target_os = "macos")]
pub fn macos_keyboard_activity() {
    macos_activity(0);
}
#[cfg(target_os = "macos")]
pub fn macos_activity(reason: u8) {
    ACTIVITY_REASON.store(reason, Ordering::SeqCst);
    KEYBOARD_REVISION.fetch_add(1, Ordering::SeqCst);
}
#[cfg(target_os = "macos")]
pub fn keyboard_revision() -> u64 {
    KEYBOARD_REVISION.load(Ordering::SeqCst)
}
#[cfg(target_os = "macos")]
pub fn macos_candidate_context() -> (u64, u64) {
    (CANDIDATE_SESSION.load(Ordering::SeqCst), keyboard_revision())
}
#[cfg(target_os = "macos")]
pub fn macos_candidate_is_current() -> bool {
    completion_enabled()
        && element::ui_automation::focus_is_available()
        && !crate::macos_ime::is_composing()
        && !SELECTING.load(Ordering::SeqCst)
        && !INSERTION_FAILED.load(Ordering::SeqCst)
        && CANDIDATE_REVISION.load(Ordering::SeqCst) == keyboard_revision()
        && INPUT_STATE.try_lock().map(|s| *s).unwrap_or(false)
        && CANDIDATE
            .try_read()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
}
#[cfg(target_os = "macos")]
pub fn queue_macos_action(key: &str, repeated: bool) -> bool {
    if !completion_enabled() { return false; }
    let Ok(config) = config::CONFIG.try_lock() else {
        return false;
    };
    let Some(config) = config.as_ref() else {
        return false;
    };
    let b = &config.keybinding;
    let choices = [
        (0, &b.exit_overlay),
        (-1, &b.accept_candidate),
        (1, &b.select_candidate_char_1),
        (2, &b.select_candidate_char_2),
        (3, &b.select_candidate_char_3),
        (4, &b.select_candidate_char_4),
        (5, &b.select_candidate_char_5),
        (6, &b.select_candidate_char_6),
        (7, &b.select_candidate_char_7),
        (8, &b.select_candidate_char_8),
        (9, &b.select_candidate_char_9),
    ];
    let Some(num) = choices
        .iter()
        .find_map(|(n, keys)| keys.iter().any(|k| k == key).then_some(*n))
    else {
        return false;
    };
    // Only partial acceptance repeats. Holding Tab/Escape must not accept or
    // dismiss a later completion, and busy repeats are dropped, never queued.
    if repeated && num <= 0 {
        return false;
    }
    if SELECTING.swap(true, Ordering::SeqCst) {
        return false;
    }
    let revision = keyboard_revision();
    let generation = TASK_GENERATION.load(Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        struct SelectionGuard;
        impl Drop for SelectionGuard {
            fn drop(&mut self) {
                SELECTING.store(false, Ordering::SeqCst);
            }
        }
        let _selection_guard = SelectionGuard;
        if completion_enabled() && revision == keyboard_revision() && generation == TASK_GENERATION.load(Ordering::SeqCst) {
            if num == 0 {
                end_overlay();
            } else {
                select_candidate(num);
            }
        }
    });
    true
}

fn start_overlay(focused_input: element::FocusedInput) {
    let _lifecycle = COMPLETION_LIFECYCLE.lock().unwrap();
    if !completion_enabled() { return; }
    info!(
        "[start_overlay] for app: {}, input: {}",
        focused_input.window_element.app, focused_input.input_element.text
    );
    set_input_state(true);
    end_overlay_stream_task();

    LAST_AI_WAS_EMPTY.store(false, Ordering::SeqCst);

    let my_generation = TASK_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    #[cfg(target_os = "macos")]
    let input_revision = keyboard_revision();
    #[cfg(target_os = "macos")]
    {
        CANDIDATE_SESSION.fetch_add(1, Ordering::SeqCst);
        CANDIDATE_REVISION.store(input_revision, Ordering::SeqCst);
        INSERTION_FAILED.store(false, Ordering::SeqCst);
        *CANDIDATE_TARGET.write().unwrap() = Some(focused_input.clone());
    }

    overlay::overlay::hide_overlay();
    *CANDIDATE.write().unwrap() = String::new();
    *SELECTED_CANDIDATE.write().unwrap() = String::new();
    let cancel_token = Arc::new(AtomicBool::new(false));
    {
        let mut token_guard = OVERLAY_CANCEL_TOKEN.lock().unwrap();
        *token_guard = Some(cancel_token.clone());
    }
    overlay::overlay::position_overlay(
        focused_input.input_element.x,
        focused_input.input_element.y,
    );

    OVERLAY_IS_RUNNING.store(true, Ordering::SeqCst);
    let handle = tauri::async_runtime::spawn(async move {
        let current_generation = TASK_GENERATION.load(Ordering::SeqCst);
        if !completion_enabled() || my_generation < current_generation {
            info!(
                "[start_overlay] Task generation {} is obsolete, current is {}. Aborting.",
                my_generation, current_generation
            );
            // Mark not running if still current
            if my_generation == current_generation {
                OVERLAY_IS_RUNNING.store(false, Ordering::SeqCst);
            }
            return;
        }

        let context = match tokio::task::spawn_blocking(move || {
            context::Context::new(&focused_input)
        })
        .await
        {
            Ok(Some(ctx)) => ctx,
            Ok(None) => {
                error!("[start_overlay] Context::new returned None.");
                if my_generation == TASK_GENERATION.load(Ordering::SeqCst) {
                    LAST_AI_WAS_EMPTY.store(true, Ordering::SeqCst);
                    OVERLAY_IS_RUNNING.store(false, Ordering::SeqCst);
                }
                return;
            }
            Err(_) => {
                error!("[start_overlay] Failed to create context in background task (JoinError).");
                if my_generation == TASK_GENERATION.load(Ordering::SeqCst) {
                    LAST_AI_WAS_EMPTY.store(true, Ordering::SeqCst);
                    OVERLAY_IS_RUNNING.store(false, Ordering::SeqCst);
                }
                return;
            }
        };

        let current_generation = TASK_GENERATION.load(Ordering::SeqCst);
        if !completion_enabled() || my_generation < current_generation {
            info!("[start_overlay] Task generation {} became obsolete during context gathering, current is {}. Aborting.", my_generation, current_generation);
            return;
        }
        #[cfg(target_os = "macos")]
        if input_revision != keyboard_revision() {
            return;
        }

        let client = ai_client::AiClient::new();
        let cancel_token_clone = cancel_token.clone();
        let result = client
            .stream_request_ai(
                context,
                move |c| {
                    if !completion_enabled() || my_generation != TASK_GENERATION.load(Ordering::SeqCst) {
                        return;
                    }
                    #[cfg(target_os = "macos")]
                    if input_revision != keyboard_revision() {
                        return;
                    }
                    let mut candidate = CANDIDATE.write().unwrap();
                    // Cancellation can happen while waiting for this lock.
                    if my_generation != TASK_GENERATION.load(Ordering::SeqCst) {
                        return;
                    }
                    #[cfg(target_os = "macos")]
                    if input_revision != keyboard_revision() {
                        return;
                    }
                    let first_visible_token = candidate.trim().is_empty();
                    candidate.push_str(&c);
                    // Whitespace-only output is not an actionable suggestion.
                    if !candidate.trim().is_empty() {
                        if first_visible_token {
                            resize_overlay_window(200.0, 40.0);
                            overlay::overlay::update_overlay(candidate.clone());
                        } else {
                            overlay::overlay::update_overlay(c);
                        }
                    }
                },
                cancel_token_clone,
            )
            .await;

        if my_generation != TASK_GENERATION.load(Ordering::SeqCst) {
            return;
        }
        #[cfg(target_os = "macos")]
        if input_revision != keyboard_revision() {
            return;
        }
        let was_cancelled = cancel_token.load(Ordering::SeqCst);
        let has_suggestion = !CANDIDATE.read().unwrap().trim().is_empty();

        match &result {
            Ok(_) if !has_suggestion && !was_cancelled => {
                info!("[start_overlay] no suggestion; waiting for changed input");
                LAST_AI_WAS_EMPTY.store(true, Ordering::SeqCst);
                CANDIDATE.write().unwrap().clear();
                overlay::overlay::hide_overlay();
            }
            Err(e) if !was_cancelled => {
                error!("[start_overlay] stream_request_ai failed: {}", e);
                LAST_AI_WAS_EMPTY.store(true, Ordering::SeqCst);
                CANDIDATE.write().unwrap().clear();
                overlay::overlay::show_request_error();
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
    TASK_GENERATION.fetch_add(1, Ordering::SeqCst);
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
    #[cfg(target_os = "macos")]
    CANDIDATE_SESSION.fetch_add(1, Ordering::SeqCst);
    set_input_state(false);
    end_overlay_stream_task();
    CANDIDATE.write().unwrap().clear();
    #[cfg(target_os = "macos")]
    {
        *CANDIDATE_TARGET.write().unwrap() = None;
        INSERTION_FAILED.store(false, Ordering::SeqCst);
    }
    overlay::overlay::hide_overlay();
}

fn select_candidate(num: i32) {
    if !completion_enabled() { return; }
    info!("[select_candidate] selecting {} chars", num);
    // Freeze the displayed completion before slicing it. Late stream tokens
    // must not race with retaining the remainder after a partial acceptance.
    #[cfg(target_os = "macos")]
    end_overlay_stream_task();
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
        let cleaned = if num == -1 {
            selected_chars.trim_end_matches(|c| c == '\r' || c == '\n')
        } else {
            selected_chars.as_str()
        };
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
    #[cfg(target_os = "macos")]
    {
        let expected = CANDIDATE_TARGET.read().unwrap().clone();
        if CANDIDATE_REVISION.load(Ordering::SeqCst) != keyboard_revision() {
            return;
        }
        let Some(expected) = expected else {
            end_overlay();
            return;
        };
        let updated = match element::ui_automation::fill_input(expected, selected_chars.clone()) {
            Ok(updated) => {
                info!("[macos] Completion insertion verified");
                updated
            }
            Err(e) => {
                log::warn!("[macos] {}", e);
                if CANDIDATE_REVISION.load(Ordering::SeqCst) == keyboard_revision() {
                    INSERTION_FAILED.store(true, Ordering::SeqCst);
                    overlay::overlay::show_insertion_error();
                } else {
                    end_overlay();
                }
                return;
            }
        };
        if CANDIDATE_REVISION.load(Ordering::SeqCst) != keyboard_revision() {
            end_overlay();
            return;
        }
        if rest_after_select.trim().is_empty() {
            end_overlay();
            return;
        }
        *CANDIDATE.write().unwrap() = rest_after_select.clone();
        SELECTED_CANDIDATE
            .write()
            .unwrap()
            .push_str(&selected_chars);
        *CANDIDATE_TARGET.write().unwrap() = Some(updated.clone());
        // The AX worker has published this verified snapshot before replying.
        // Advance the listener baseline too, so our own caret movement doesn't
        // discard the remainder and request an unrelated new completion.
        *FORMER_FOCUSED_INPUT.write().unwrap() = Some(updated.clone());
        overlay::overlay::position_overlay(updated.input_element.x, updated.input_element.y);
        overlay::overlay::replace_overlay(rest_after_select.clone());
        info!(
            "[select_candidate] accepted_chars={} remaining_chars={}",
            selected_num,
            rest_after_select.chars().count()
        );
    }
    #[cfg(target_os = "windows")]
    {
        element::fill_input(selected_chars.clone());

        {
            let mut current_selected_candidate = SELECTED_CANDIDATE.write().unwrap();
            *current_selected_candidate =
                format!("{}{}", current_selected_candidate, selected_chars);
        }
        {
            let mut candidate = CANDIDATE.write().unwrap();
            *candidate = rest_after_select;
        }

        debug!("[select_candidate] call overlay::overlay::select_candidate");
        overlay::overlay::select_candidate(selected_num);
    }
}

fn save_history(focused_input: &element::FocusedInput) {
    info!(
        "[save_history] for app: {}",
        focused_input.window_element.app
    );
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
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
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
        #[cfg(target_os = "macos")]
        let mut observed_revision = keyboard_revision();
        #[cfg(target_os = "macos")]
        let mut last_activity = std::time::Instant::now();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(focus_interval));
            if !completion_enabled() { continue; }
            #[cfg(target_os = "macos")]
            {
                if SELECTING.load(Ordering::SeqCst) {
                    continue;
                }
                let revision = keyboard_revision();
                if observed_revision != revision {
                    observed_revision = revision;
                    last_activity = std::time::Instant::now();
                    if get_input_state() {
                        let reason = match ACTIVITY_REASON.load(Ordering::SeqCst) {
                            1 => "mouse", 2 => "modifiers", 3 => "tap_reset", 4 => "input_source", _ => "keyboard",
                        };
                        info!("[completion] dismissed reason={reason}");
                    }
                    end_overlay();
                    *FORMER_FOCUSED_INPUT.write().unwrap() = None;
                }
                let cfg = config::get_config().unwrap();
                let configured = match cfg.ai_client.provider {
                    config::ai_client::AiProvider::API => !cfg.ai_client.api_key.trim().is_empty(),
                    config::ai_client::AiProvider::CMD => !cfg.ai_client.cmd.trim().is_empty(),
                };
                if !configured
                    || crate::macos_ime::is_composing()
                    || !hook::is_ready()
                    || last_activity.elapsed() < std::time::Duration::from_millis(250)
                {
                    continue;
                }
            }
            let current_focus = element::get_current_focus_info();
            #[cfg(target_os = "macos")]
            if current_focus.is_some() && !element::ui_automation::focus_is_available() {
                continue;
            }
            match current_focus {
                Some(focused_input) => {
                    // Decide actions while holding the lock, but perform heavy work after releasing it
                    let (should_start_overlay, save_former_opt) = {
                        let mut guard = FORMER_FOCUSED_INPUT.write().unwrap();
                        if let Some(former_focused_input) = guard.as_ref() {
                            if !former_focused_input.eq(&focused_input) {
                                info!("[completion] restarting reason=control_or_selection_changed");
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
                                            let selected_candidate =
                                                SELECTED_CANDIDATE.read().unwrap();
                                            let full_candidate =
                                                format!("{}{}", selected_candidate, candidate);
                                            let candidate_chars: Vec<char> =
                                                full_candidate.chars().collect();
                                            let new_content_chars: Vec<char> =
                                                new_content.chars().collect();
                                            let mut matched = false;
                                            for i in 0..candidate_chars.len() {
                                                if new_content_chars.len() <= i {
                                                    break;
                                                }
                                                let candidate_prefix: String =
                                                    candidate_chars[..=i].iter().collect();
                                                let new_content_suffix: String = new_content_chars
                                                    [new_content_chars.len() - i - 1..]
                                                    .iter()
                                                    .collect();
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
                                    debug!(
                                        "[listen_input_state] restart_overlay: {}",
                                        restart_overlay
                                    );
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
                                    // An empty/error response is a completed attempt, not
                                    // a reason to call the provider every polling interval.
                                    // Real input/focus changes above may start a new request.
                                    let should_force_restart = get_input_state()
                                        && CANDIDATE.read().unwrap().is_empty()
                                        && !OVERLAY_IS_RUNNING.load(Ordering::SeqCst)
                                        && !LAST_AI_WAS_EMPTY.load(Ordering::SeqCst);
                                    *guard = Some(focused_input.clone());
                                    if should_force_restart {
                                        info!("[listen_input_state] content unchanged; in input without candidates or running task, restarting overlay");
                                        (true, None)
                                    } else {
                                        debug!("[listen_input_state] content unchanged; no need to restart overlay");
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
                        #[cfg(target_os = "macos")]
                        if SELECTING.load(Ordering::SeqCst) {
                            continue;
                        }
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
                            error!(
                                "[listen_input_state] save_history panicked on focus lost: {:?}",
                                e
                            );
                        }
                    }
                    debug!("[listen_input_state] focused_input: None");
                    end_overlay();
                }
            }
        }
    });
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::os::{
        element::types::{FocusedInput, TextRange, UIElement},
        WindowElement,
    };
    use std::time::{Duration, Instant};

    fn wait_for_selection() {
        let deadline = Instant::now() + Duration::from_secs(2);
        while SELECTING.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline, "acceptance did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn press_one(repeated: bool) -> bool {
        hook::consume_key_down(18, false, repeated, || Some("1".into()))
    }

    #[test]
    fn held_partial_acceptance_repeats_without_queueing_or_crossing_completions() {
        // Only the AX worker is replaced; exercise the real event routing and
        // async selection path. No keyboard events are posted to the desktop.
        let fixture_config = toml::from_str(include_str!("../../config_macos.toml")).unwrap();
        let original_config = config::CONFIG.lock().unwrap().replace(fixture_config);
        let focus = FocusedInput {
            selection: TextRange {
                location: 3,
                length: 0,
            },
            window_element: WindowElement {
                id: 1,
                app: "Fixture".into(),
                title: "Fixture".into(),
                class_name: "AXTextArea".into(),
                x: 0,
                y: 0,
                width: 500,
                height: 300,
            },
            input_element: UIElement {
                id: "1:2".into(),
                text: "".into(),
                x: 30,
                y: 100,
                z: 0,
                width: 400,
                height: 100,
                window_id: 1,
                control_type: 50004,
                element_type: 0,
                content: "abcTAIL".into(),
            },
        };
        let worker = element::ui_automation::mock_insertion_worker(focus.clone(), 3);
        set_input_state(true);
        CANDIDATE_REVISION.store(keyboard_revision(), Ordering::SeqCst);
        *CANDIDATE_TARGET.write().unwrap() = Some(focus.clone());
        *CANDIDATE.write().unwrap() = "甲😀乙".into();

        assert!(press_one(false));
        wait_for_selection();
        assert!(get_input_state());
        assert_eq!(CANDIDATE.read().unwrap().as_str(), "😀乙");
        assert_eq!(
            CANDIDATE_TARGET
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .selection
                .location,
            4
        );
        assert_eq!(
            FORMER_FOCUSED_INPUT
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .input_element
                .content,
            "abc甲TAIL"
        );

        // Repeats during insertion are consumed but never accumulated for later.
        let revision = keyboard_revision();
        SELECTING.store(true, Ordering::SeqCst);
        for _ in 0..10 {
            assert!(press_one(true));
        }
        SELECTING.store(false, Ordering::SeqCst);
        assert_eq!(keyboard_revision(), revision);
        assert_eq!(CANDIDATE.read().unwrap().as_str(), "😀乙");

        assert!(press_one(true));
        wait_for_selection();
        assert!(get_input_state());
        assert_eq!(CANDIDATE.read().unwrap().as_str(), "乙");
        assert_eq!(
            CANDIDATE_TARGET
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .selection
                .location,
            6
        );

        // A replacement completion cannot be accepted by an already held key.
        CANDIDATE_SESSION.fetch_add(1, Ordering::SeqCst);
        assert!(press_one(true));
        assert!(!SELECTING.load(Ordering::SeqCst));
        assert_eq!(CANDIDATE.read().unwrap().as_str(), "乙");

        assert!(hook::consume_key_up(18));
        assert!(!hook::consume_key_up(18));
        assert!(!SELECTING.load(Ordering::SeqCst));
        // A normal new press can still accept the remainder with Tab.
        assert!(hook::consume_key_down(48, false, false, || Some("Tab".into())));
        wait_for_selection();
        assert!(!get_input_state());
        assert!(CANDIDATE.read().unwrap().is_empty());
        assert!(CANDIDATE_TARGET.read().unwrap().is_none());
        assert_eq!(
            worker.join().unwrap().input_element.content,
            "abc甲😀乙TAIL"
        );
        // Keep consuming repeats after completion, without leaking Tab/1 or
        // treating an unclaimed repeat as a new completion shortcut.
        assert!(hook::consume_key_down(48, false, true, || Some("Tab".into())));
        assert!(!SELECTING.load(Ordering::SeqCst));
        assert!(hook::consume_key_up(48));
        assert!(!press_one(true));
        assert!(!SELECTING.load(Ordering::SeqCst));

        // Pausing cancels an in-flight stream, clears the candidate and blocks
        // both automatic restart and key acceptance until explicitly resumed.
        set_input_state(true);
        *CANDIDATE.write().unwrap() = "pending completion".into();
        let token = Arc::new(AtomicBool::new(false));
        *OVERLAY_CANCEL_TOKEN.lock().unwrap() = Some(token.clone());
        *OVERLAY_TASK_HANDLE.lock().unwrap() = Some(tauri::async_runtime::spawn(std::future::pending::<()>()));
        OVERLAY_IS_RUNNING.store(true, Ordering::SeqCst);
        config::CONFIG.lock().unwrap().as_mut().unwrap().system.completion_enabled = false;
        sync_completion_enabled();
        assert!(!completion_enabled());
        assert!(!get_input_state());
        assert!(token.load(Ordering::SeqCst));
        assert!(CANDIDATE.read().unwrap().is_empty());
        assert!(CANDIDATE_TARGET.read().unwrap().is_none());
        assert!(OVERLAY_TASK_HANDLE.lock().unwrap().is_none());
        assert!(!queue_macos_action("1", false));
        start_overlay(focus);
        assert!(!OVERLAY_IS_RUNNING.load(Ordering::SeqCst));
        config::CONFIG.lock().unwrap().as_mut().unwrap().system.completion_enabled = true;
        sync_completion_enabled();
        assert!(completion_enabled());
        assert!(FORMER_FOCUSED_INPUT.read().unwrap().is_none());
        *config::CONFIG.lock().unwrap() = original_config;
    }
}

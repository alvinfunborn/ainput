//! Read NSPasteboard on the main thread; retain only bounded plain-text history.
use core_foundation::{base::TCFType, string::CFString};
use once_cell::sync::Lazy;
use std::{
    collections::VecDeque,
    ffi::{c_char, c_void},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};

static HISTORY: Lazy<Mutex<VecDeque<String>>> = Lazy::new(|| Mutex::new(VecDeque::new()));
static RUNNING: AtomicBool = AtomicBool::new(false);
static PENDING: AtomicBool = AtomicBool::new(false);
static CHANGE_COUNT: Mutex<Option<isize>> = Mutex::new(None);

#[link(name = "objc")]
extern "C" {
    fn objc_msgSend();
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *const c_void;
}
unsafe fn selector(name: &'static [u8]) -> *const c_void {
    sel_registerName(name.as_ptr().cast())
}

fn poll() {
    if !crate::input::completion_enabled() { return; }
    crate::macos_ime::refresh_source();
    unsafe {
        let object: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const ());
        let number: unsafe extern "C" fn(*mut c_void, *const c_void) -> isize =
            std::mem::transmute(objc_msgSend as *const ());
        let get_string: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_void,
        ) -> *const c_void = std::mem::transmute(objc_msgSend as *const ());
        let board = object(
            objc_getClass(b"NSPasteboard\0".as_ptr().cast()),
            selector(b"generalPasteboard\0"),
        );
        if board.is_null() {
            return;
        }
        let count = number(board, selector(b"changeCount\0"));
        let mut last = CHANGE_COUNT.lock().unwrap();
        if *last == Some(count) {
            return;
        }
        *last = Some(count);
        let kind = CFString::new("public.utf8-plain-text");
        let value = get_string(board, selector(b"stringForType:\0"), kind.as_CFTypeRef());
        if value.is_null() {
            return;
        }
        let text = CFString::wrap_under_get_rule(value.cast()).to_string();
        add_history(&mut HISTORY.lock().unwrap(), &text);
    }
}
fn add_history(history: &mut VecDeque<String>, text: &str) {
    let text: String = text.chars().take(1024).collect();
    if text.is_empty() || history.back() == Some(&text) {
        return;
    }
    history.push_back(text);
    while history.iter().map(|s| s.chars().count()).sum::<usize>() > 1024 {
        history.pop_front();
    }
}
pub fn get_clipboard_history() -> Vec<String> {
    HISTORY.lock().unwrap().iter().cloned().collect()
}
pub fn init(app: tauri::AppHandle) {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        while RUNNING.load(Ordering::Acquire) {
            if crate::macos_access::is_accessibility_trusted()
                && !PENDING.swap(true, Ordering::AcqRel)
            {
                if app
                    .run_on_main_thread(|| {
                        if RUNNING.load(Ordering::Acquire) {
                            poll();
                        }
                        PENDING.store(false, Ordering::Release);
                    })
                    .is_err()
                {
                    PENDING.store(false, Ordering::Release);
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}
pub fn cleanup() {
    RUNNING.store(false, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_is_unicode_safe_bounded_and_deduplicated() {
        let mut history = VecDeque::new();
        add_history(&mut history, &"你好😀".repeat(600));
        assert_eq!(history[0].chars().count(), 1024);
        add_history(&mut history, "new");
        add_history(&mut history, "new");
        assert_eq!(history, VecDeque::from(["new".to_string()]));
    }
}

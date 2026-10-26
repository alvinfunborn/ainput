//! Active event tap, following Screen Buoy's dedicated-run-loop design.
//! The callback never performs AX IPC, network work or text insertion.
use core_foundation::{
    base::TCFType,
    mach_port::CFMachPort,
    runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop},
};
use core_graphics::event::EventField;
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};

type Event = *const c_void;
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: unsafe extern "C" fn(Event, u32, Event, Event) -> Event,
        info: Event,
    ) -> core_foundation::mach_port::CFMachPortRef;
    fn CGEventTapEnable(tap: core_foundation::mach_port::CFMachPortRef, enabled: bool);
    fn CGEventTapIsEnabled(tap: core_foundation::mach_port::CFMachPortRef) -> bool;
    fn CGEventGetFlags(event: Event) -> u64;
    fn CGEventGetIntegerValueField(event: Event, field: u32) -> i64;
    fn CGEventKeyboardGetUnicodeString(
        event: Event,
        max: usize,
        actual: *mut usize,
        text: *mut u16,
    );
}
static RUNNING: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);
// Keep a retained run loop, including during cleanup (no dangling raw pointer).
static RUN_LOOP: Mutex<Option<CFRunLoop>> = Mutex::new(None);
#[derive(Clone)]
struct ConsumedKey {
    name: String,
    context: (u64, u64),
}
thread_local! {
    static PORT: RefCell<Option<CFMachPort>> = const { RefCell::new(None) };
    static SWALLOWED: RefCell<HashMap<u16, ConsumedKey>> = RefCell::new(HashMap::new());
}
pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

fn has_modifiers(flags: u64) -> bool {
    flags & (0x20000 | 0x40000 | 0x80000 | 0x100000) != 0
}
fn key_name(event: Event, code: u16) -> Option<String> {
    if let Ok(map) = crate::config::keyboard::VIRTUAL_KEY_MAP.try_lock() {
        if let Some(name) = map.get(&code) {
            return Some(name.clone());
        }
    }
    let mut text = [0u16; 8];
    let mut len = 0;
    unsafe {
        CGEventKeyboardGetUnicodeString(event, text.len(), &mut len, text.as_mut_ptr());
    }
    let text = String::from_utf16(&text[..len.min(text.len())]).ok()?;
    (!text.is_empty() && !text.chars().any(char::is_control)).then(|| text.to_uppercase())
}

unsafe extern "C" fn callback(_proxy: Event, kind: u32, event: Event, _info: Event) -> Event {
    // Never unwind over the C callback boundary. A failed handler passes input.
    std::panic::catch_unwind(|| handle_event(kind, event)).unwrap_or(event)
}
fn handle_event(kind: u32, event: Event) -> Event {
    if kind == 0xffff_fffe || kind == 0xffff_ffff {
        READY.store(false, Ordering::Release);
        SWALLOWED.with(|s| s.borrow_mut().clear());
        super::macos_activity(3);
        PORT.with(|p| {
            if let Some(port) = p.borrow().as_ref() {
                unsafe {
                    CGEventTapEnable(port.as_concrete_TypeRef(), true);
                    READY.store(
                        CGEventTapIsEnabled(port.as_concrete_TypeRef()),
                        Ordering::Release,
                    );
                }
            }
        });
        return event;
    }
    if event.is_null() {
        return event;
    }
    if unsafe { CGEventGetIntegerValueField(event, EventField::EVENT_SOURCE_USER_DATA) }
        == crate::os::element::ui_automation::INJECTED_EVENT_TAG
    {
        return event;
    }
    // Mouse clicks and modifier transitions invalidate a displayed completion.
    if matches!(kind, 1 | 3 | 25) {
        crate::macos_ime::clicked();
    }
    if matches!(kind, 1 | 3 | 25 | 12) {
        super::macos_activity(if kind == 12 { 2 } else { 1 });
        return event;
    }
    let code =
        unsafe { CGEventGetIntegerValueField(event, EventField::KEYBOARD_EVENT_KEYCODE) } as u16;
    if kind == 11 {
        return if consume_key_up(code) {
            std::ptr::null()
        } else {
            event
        };
    }
    if kind != 10 {
        return event;
    }
    let flags = unsafe { CGEventGetFlags(event) };
    let repeated =
        unsafe { CGEventGetIntegerValueField(event, EventField::KEYBOARD_EVENT_AUTOREPEAT) } != 0;
    if consume_key_down(code, has_modifiers(flags), repeated, || key_name(event, code)) {
        std::ptr::null()
    } else {
        event
    }
}

pub(super) fn consume_key_up(code: u16) -> bool {
    SWALLOWED.with(|s| s.borrow_mut().remove(&code).is_some())
}

pub(super) fn consume_key_down(
    code: u16,
    modified: bool,
    repeated: bool,
    key_name: impl FnOnce() -> Option<String>,
) -> bool {
    // macOS sends repeated key-downs before a single key-up. Keep consuming
    // the entire press, but allow partial acceptance to advance on each repeat.
    // Never let a busy/exhausted/cancelled press leak literal digits into input.
    if let Some(held) = SWALLOWED.with(|s| s.borrow().get(&code).cloned()) {
        if repeated
            && !modified
            && held.context == super::macos_candidate_context()
            && super::macos_candidate_is_current()
        {
            super::queue_macos_action(&held.name, true);
        }
        return true;
    }
    if !modified && !repeated && super::macos_candidate_is_current() {
        if let Some(key) = key_name() {
            let context = super::macos_candidate_context();
            if super::queue_macos_action(&key, false) {
                SWALLOWED.with(|s| {
                    s.borrow_mut().insert(code, ConsumedKey { name: key, context });
                });
                return true;
            }
        }
    }
    crate::macos_ime::key_down(code, modified);
    super::macos_keyboard_activity();
    false
}

pub fn init(_app: tauri::AppHandle) {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::Builder::new()
        .name("keyboard-tap".into())
        .spawn(|| {
            let mask = [1, 3, 25, 10, 11, 12]
                .iter()
                .fold(0u64, |m, bit| m | (1 << bit));
            while RUNNING.load(Ordering::Acquire) {
                let raw = unsafe { CGEventTapCreate(0, 0, 0, mask, callback, std::ptr::null()) };
                if raw.is_null() {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
                let port = unsafe { CFMachPort::wrap_under_create_rule(raw) };
                let Ok(source) = port.create_runloop_source(0) else {
                    break;
                };
                let run_loop = CFRunLoop::get_current();
                run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
                unsafe {
                    CGEventTapEnable(port.as_concrete_TypeRef(), true);
                }
                READY.store(
                    unsafe { CGEventTapIsEnabled(port.as_concrete_TypeRef()) },
                    Ordering::Release,
                );
                PORT.with(|p| *p.borrow_mut() = Some(port));
                *RUN_LOOP.lock().unwrap() = Some(run_loop.clone());
                log::info!("[macos] Keyboard event tap connected");
                // Bounded run-loop slices also make shutdown safe before run starts.
                while RUNNING.load(Ordering::Acquire) {
                    CFRunLoop::run_in_mode(
                        unsafe { kCFRunLoopDefaultMode },
                        Duration::from_millis(500),
                        false,
                    );
                }
                READY.store(false, Ordering::Release);
                *RUN_LOOP.lock().unwrap() = None;
                PORT.with(|p| {
                    if let Some(port) = p.borrow_mut().take() {
                        unsafe {
                            CGEventTapEnable(port.as_concrete_TypeRef(), false);
                        }
                    }
                });
                run_loop.remove_source(&source, unsafe { kCFRunLoopCommonModes });
            }
            RUNNING.store(false, Ordering::Release);
        })
        .expect("Could not start keyboard event tap");
}
pub fn cleanup() {
    RUNNING.store(false, Ordering::Release);
    READY.store(false, Ordering::Release);
    if let Some(run_loop) = RUN_LOOP.lock().unwrap().as_ref() {
        run_loop.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_option_control_and_shift_combinations_pass_through() {
        for flags in [0x20000, 0x40000, 0x80000, 0x100000, 0x120000] {
            assert!(has_modifiers(flags));
        }
        assert!(!has_modifiers(0));
        assert!(!has_modifiers(0x10000)); // Caps Lock is not a shortcut modifier.
    }
}

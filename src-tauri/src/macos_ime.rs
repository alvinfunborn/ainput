//! Conservative IME guard: AX does not expose marked text consistently across
//! applications. Wait for Space/Enter/Escape or a click before offering another
//! completion after typing with a non-ASCII input source.
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    string::CFStringRef,
};
use std::sync::atomic::{AtomicBool, Ordering};

static IME: AtomicBool = AtomicBool::new(false);
static COMPOSING: AtomicBool = AtomicBool::new(false);
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> CFTypeRef;
    fn TISGetInputSourceProperty(source: CFTypeRef, key: CFStringRef) -> CFTypeRef;
    static kTISPropertyInputSourceIsASCIICapable: CFStringRef;
}

/// Called on the main thread with the clipboard polling task.
pub fn refresh_source() {
    unsafe {
        let source = TISCopyCurrentKeyboardInputSource();
        if source.is_null() {
            return;
        }
        let source = CFType::wrap_under_create_rule(source);
        let property =
            TISGetInputSourceProperty(source.as_CFTypeRef(), kTISPropertyInputSourceIsASCIICapable);
        if property.is_null() {
            return;
        }
        let property = CFType::wrap_under_get_rule(property);
        let ime = property
            .downcast::<CFBoolean>()
            .map(|b| !bool::from(b))
            .unwrap_or(true);
        if IME.swap(ime, Ordering::SeqCst) != ime {
            log::debug!("[macos] Input source requires composition guard: {ime}");
            // Selecting an IME is not itself composition. The first unmodified
            // typing event activates the guard; source switches end the old
            // source's composition. This also permits pasted, committed text.
            COMPOSING.store(false, Ordering::SeqCst);
            crate::input::macos_activity(4);
        }
    }
}
pub fn is_composing() -> bool {
    COMPOSING.load(Ordering::SeqCst)
}
pub fn clicked() {
    COMPOSING.store(false, Ordering::SeqCst);
    log::debug!("[macos] Composition guard cleared by click");
}
pub fn key_down(code: u16, modified: bool) {
    let next = next_state(IME.load(Ordering::SeqCst), is_composing(), code, modified);
    if COMPOSING.swap(next, Ordering::SeqCst) != next {
        log::debug!("[macos] Composition guard active: {next}");
    }
}
fn next_state(ime: bool, composing: bool, code: u16, modified: bool) -> bool {
    if !ime {
        return false;
    }
    if modified {
        return composing;
    }
    match code {
        36 | 49 | 53 | 76 => false, // Enter, Space, Escape, keypad Enter
        0..=47 | 50 | 51 => true,   // typing/backspace can update marked text
        _ => composing,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ime_selection_keys_are_not_completion_shortcuts_during_composition() {
        assert!(next_state(true, false, 45, false)); // N starts composition
        assert!(next_state(true, true, 48, false)); // Tab keeps composing
        assert!(next_state(true, true, 18, false)); // 1 belongs to the IME
        assert!(!next_state(true, true, 49, false)); // Space commits
        assert!(!next_state(true, true, 36, false)); // Enter commits
        assert!(!next_state(false, false, 45, false));
    }
}

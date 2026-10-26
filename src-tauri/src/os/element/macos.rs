//! Accessibility objects stay on one worker thread. Only value snapshots cross
//! threads; accepting a completion always revalidates the native element.
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    string::{CFString, CFStringRef},
};
use core_graphics::{
    event::{CGEvent, CGEventFlags, EventField},
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::{CGPoint, CGRect, CGSize},
};
use once_cell::sync::Lazy;
use std::{
    ffi::c_void,
    sync::{atomic::{AtomicBool, Ordering}, mpsc, Mutex},
    time::{Duration, Instant},
};

thread_local! {
    static ENABLED_APPS: std::cell::RefCell<std::collections::HashSet<i32>> = std::cell::RefCell::new(std::collections::HashSet::new());
}

pub use super::types::{FocusedInput, TextRange, UIElement};
use crate::os::WindowElement;

pub const INJECTED_EVENT_TAG: i64 = 0x41494e505554;
const AX_TIMEOUT: f32 = 0.05;
const MAX_TEXT_UNITS: usize = 64 * 1024;
static FOCUS_AVAILABLE: AtomicBool = AtomicBool::new(false);

pub fn focus_is_available() -> bool {
    FOCUS_AVAILABLE.load(Ordering::Acquire)
}

#[derive(Default)]
struct FocusReadRecovery {
    unavailable_since: Option<Instant>,
}
impl FocusReadRecovery {
    fn retain_last_focus(&mut self, now: Instant) -> bool {
        now.duration_since(*self.unavailable_since.get_or_insert(now)) < Duration::from_millis(500)
    }
    fn recovered(&mut self) {
        if let Some(since) = self.unavailable_since.take() {
            log::info!("[macos_focus] AX reads recovered after {}ms", since.elapsed().as_millis());
        }
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CFRange {
    location: isize,
    length: isize,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        el: CFTypeRef,
        attr: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementCopyParameterizedAttributeValue(
        el: CFTypeRef,
        attr: CFStringRef,
        param: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(el: CFTypeRef, attr: CFStringRef, value: CFTypeRef) -> i32;
    fn AXUIElementSetMessagingTimeout(el: CFTypeRef, timeout: f32) -> i32;
    fn AXUIElementGetPid(el: CFTypeRef, pid: *mut i32) -> i32;
    fn AXValueGetType(value: CFTypeRef) -> u32;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, out: *mut c_void) -> u8;
}
#[link(name = "proc")]
extern "C" {
    fn proc_name(pid: i32, buffer: *mut c_void, size: u32) -> i32;
}

fn attr(el: &CFType, name: &str) -> Option<CFType> {
    let mut value = std::ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(
            el.as_CFTypeRef(),
            CFString::new(name).as_concrete_TypeRef(),
            &mut value,
        )
    };
    if status != 0 {
        log::trace!("[macos] AX attribute {name} unavailable ({status})");
    }
    (status == 0 && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
}
fn string(el: &CFType, name: &str) -> Option<String> {
    attr(el, name)?
        .downcast::<CFString>()
        .map(|s| s.to_string())
}
fn boolean(el: &CFType, name: &str) -> Option<bool> {
    attr(el, name)?
        .downcast::<CFBoolean>()
        .map(|b| bool::from(b))
}
fn value<T: Default>(v: &CFType, kind: u32) -> Option<T> {
    let mut out = T::default();
    (unsafe {
        AXValueGetType(v.as_CFTypeRef()) == kind
            && AXValueGetValue(v.as_CFTypeRef(), kind, (&mut out as *mut T).cast()) != 0
    })
    .then_some(out)
}
fn rect(el: &CFType) -> Option<CGRect> {
    let p = value::<CGPoint>(&attr(el, "AXPosition")?, 1)?;
    let s = value::<CGSize>(&attr(el, "AXSize")?, 2)?;
    Some(CGRect::new(&p, &s))
}
fn pid(el: &CFType) -> Option<i32> {
    let mut pid = 0;
    (unsafe { AXUIElementGetPid(el.as_CFTypeRef(), &mut pid) } == 0 && pid > 0).then_some(pid)
}
fn process_name(pid: i32) -> String {
    let mut name = [0u8; 1024];
    let len = unsafe { proc_name(pid, name.as_mut_ptr().cast(), name.len() as u32) };
    if len > 0 {
        String::from_utf8_lossy(&name[..len as usize])
            .trim_end_matches('\0')
            .to_string()
    } else {
        pid.to_string()
    }
}

struct NativeFocus {
    app: CFType,
    window: CFType,
    element: CFType,
    pid: i32,
    snapshot: FocusedInput,
}

fn read_focus(
    system: &CFType,
    previous: Option<&NativeFocus>,
    next_id: &mut u64,
) -> Result<Option<NativeFocus>, &'static str> {
    if !crate::macos_access::is_accessibility_trusted() {
        return Ok(None);
    }
    let app = attr(system, "AXFocusedApplication").ok_or("focused_application")?;
    let owner = pid(&app).ok_or("application_pid")?;
    if owner == std::process::id() as i32 {
        return Ok(None);
    }
    unsafe {
        AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), AX_TIMEOUT);
    }
    let name = process_name(owner);
    let cfg = crate::config::get_config().ok_or("configuration")?;
    if cfg
        .ui_automation
        .ignore_apps
        .iter()
        .any(|s| s.eq_ignore_ascii_case(&name))
    {
        return Ok(None);
    }
    // Chromium/Electron may not expose text fields until an AX client enables
    // its accessibility tree. Screen Buoy uses the same enhanced-UI request.
    ENABLED_APPS.with(|seen| {
        if seen.borrow_mut().insert(owner) {
            for attribute in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
                unsafe {
                    AXUIElementSetAttributeValue(
                        app.as_CFTypeRef(),
                        CFString::new(attribute).as_concrete_TypeRef(),
                        CFBoolean::true_value().as_CFTypeRef(),
                    );
                }
            }
        }
    });
    let window = attr(&app, "AXFocusedWindow").ok_or("focused_window")?;
    let element = attr(&app, "AXFocusedUIElement").ok_or("focused_element")?;
    unsafe {
        AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), AX_TIMEOUT);
    }
    if pid(&element).ok_or("element_pid")? != owner
        || boolean(&element, "AXEnabled") == Some(false)
        || boolean(&element, "AXEditable") == Some(false)
    {
        return Ok(None);
    }
    let role = string(&element, "AXRole").ok_or("element_role")?;
    let subrole = string(&element, "AXSubrole").unwrap_or_default();
    // Never read AXValue for password/protected controls or arbitrary web areas.
    if !editable_role(
        &role,
        &subrole,
        boolean(&element, "AXProtectedContent").unwrap_or(false),
    ) {
        return Ok(None);
    }
    let range_value = attr(&element, "AXSelectedTextRange").ok_or("selected_range")?;
    let range = value::<CFRange>(&range_value, 4).ok_or("selected_range_value")?;
    if range.location < 0 || range.length != 0 {
        return Ok(None);
    }
    let content = string(&element, "AXValue").ok_or("input_value")?;
    if content.encode_utf16().count() > MAX_TEXT_UNITS {
        return Ok(None);
    }
    let selection = TextRange {
        location: range.location as usize,
        length: 0,
    };
    utf16_byte_index(&content, selection.location).ok_or("selection_bounds")?;
    let window_rect = rect(&window).ok_or("window_bounds")?;
    let input_rect = rect(&element).ok_or("input_bounds")?;
    if input_rect.size.width <= 0.0 || input_rect.size.height <= 0.0 {
        return Ok(None);
    }
    let mut caret_value = std::ptr::null();
    let caret_status = unsafe {
        AXUIElementCopyParameterizedAttributeValue(
            element.as_CFTypeRef(),
            CFString::new("AXBoundsForRange").as_concrete_TypeRef(),
            range_value.as_CFTypeRef(),
            &mut caret_value,
        )
    };
    let caret = if caret_status == 0 && !caret_value.is_null() {
        value::<CGRect>(&unsafe { CFType::wrap_under_create_rule(caret_value) }, 3)
            .filter(|r| r.size.height > 0.0 && r.origin.x.is_finite() && r.origin.y.is_finite())
            .unwrap_or(input_rect)
    } else {
        input_rect
    };
    let same = previous.filter(|p| p.pid == owner && p.window == window && p.element == element);
    let id = same
        .map(|p| p.snapshot.input_element.id.clone())
        .unwrap_or_else(|| {
            *next_id += 1;
            format!("{owner}:{}", *next_id)
        });
    let window_id = previous
        .filter(|p| p.pid == owner && p.window == window)
        .map(|p| p.snapshot.window_element.id)
        .unwrap_or_else(|| {
            *next_id += 1;
            *next_id as i64
        });
    let snapshot = FocusedInput {
        selection,
        window_element: WindowElement {
            id: window_id,
            app: name,
            title: string(&window, "AXTitle").unwrap_or_default(),
            class_name: role.clone(),
            x: window_rect.origin.x.round() as i32,
            y: window_rect.origin.y.round() as i32,
            width: window_rect.size.width.round() as i32,
            height: window_rect.size.height.round() as i32,
        },
        input_element: UIElement {
            id,
            text: string(&element, "AXTitle").unwrap_or_default(),
            content,
            x: caret.origin.x.round() as i32,
            y: caret.origin.y.round() as i32,
            z: 0,
            width: input_rect.size.width.round() as i32,
            height: input_rect.size.height.round() as i32,
            window_id,
            control_type: 50004,
            element_type: 0,
        },
    };
    Ok(Some(NativeFocus {
        app,
        window,
        element,
        pid: owner,
        snapshot,
    }))
}

fn editable_role(role: &str, subrole: &str, protected: bool) -> bool {
    !protected
        && subrole != "AXSecureTextField"
        && matches!(role, "AXTextField" | "AXTextArea" | "AXComboBox")
}

pub fn utf16_byte_index(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (index, ch) in text.char_indices() {
        if units == offset {
            return Some(index);
        }
        units += ch.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}
pub fn text_before_selection(focus: &FocusedInput) -> Option<String> {
    Some(
        focus.input_element.content
            [..utf16_byte_index(&focus.input_element.content, focus.selection.location)?]
            .to_string(),
    )
}
fn inserted_text(text: &str, range: TextRange, insertion: &str) -> Option<String> {
    let start = utf16_byte_index(text, range.location)?;
    let end = utf16_byte_index(text, range.location.checked_add(range.length)?)?;
    Some(format!("{}{}{}", &text[..start], insertion, &text[end..]))
}

// Keep keyboard payloads small for AppKit/Chromium's Unicode event handling.
// Count UTF-16 units, not bytes or scalar values, and never split a surrogate pair.
fn unicode_chunks(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let (mut start, mut units) = (0, 0);
    for (index, ch) in text.char_indices() {
        if units + ch.len_utf16() > 20 {
            chunks.push(&text[start..index]);
            start = index;
            units = 0;
        }
        units += ch.len_utf16();
    }
    if start < text.len() {
        chunks.push(&text[start..]);
    }
    chunks
}

fn focus_is_current(system: &CFType, focus: &NativeFocus, revision: u64) -> bool {
    crate::input::completion_enabled()
        && crate::input::keyboard_revision() == revision
        && attr(system, "AXFocusedApplication").as_ref() == Some(&focus.app)
        && attr(&focus.app, "AXFocusedWindow").as_ref() == Some(&focus.window)
        && attr(&focus.app, "AXFocusedUIElement").as_ref() == Some(&focus.element)
}

fn verify_value(
    focus: &NativeFocus,
    expected: &str,
    selection: TextRange,
    until: Instant,
) -> Result<(), String> {
    let mut observed = None;
    while Instant::now() < until {
        observed = string(&focus.element, "AXValue");
        let range =
            attr(&focus.element, "AXSelectedTextRange").and_then(|v| value::<CFRange>(&v, 4));
        if observed.as_deref() == Some(expected)
            && range.is_some_and(|r| {
                r.location == selection.location as isize && r.length == selection.length as isize
            })
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    log::warn!(
        "[macos_insert] unconfirmed value_readable={} value_changed={} observed_units={:?} expected_units={}",
        observed.is_some(),
        observed.as_ref().map(|s| s != &focus.snapshot.input_element.content).unwrap_or(false),
        observed.as_ref().map(|s| s.encode_utf16().count()),
        expected.encode_utf16().count(),
    );
    Err("Insertion sent, but the input value could not be verified; not retried".into())
}

struct FillRequest {
    revision: u64,
    expected: FocusedInput,
    text: String,
    deadline: Instant,
    reply: mpsc::SyncSender<Result<FocusedInput, String>>,
}
static FILL_SENDER: Lazy<Mutex<Option<mpsc::SyncSender<FillRequest>>>> =
    Lazy::new(|| Mutex::new(None));

pub fn fill_input(expected: FocusedInput, text: String) -> Result<FocusedInput, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    let request = FillRequest {
        revision: crate::input::keyboard_revision(),
        expected,
        text,
        deadline: Instant::now() + Duration::from_millis(800),
        reply: tx,
    };
    FILL_SENDER
        .lock()
        .map_err(|_| "Accessibility worker unavailable")?
        .as_ref()
        .ok_or("Accessibility worker not started")?
        .try_send(request)
        .map_err(|_| "Accessibility worker busy")?;
    rx.recv_timeout(Duration::from_secs(4))
        .map_err(|_| "Text insertion could not be confirmed".to_string())?
}

fn insert(
    system: &CFType,
    previous: Option<&NativeFocus>,
    next_id: &mut u64,
    request: &FillRequest,
) -> Result<NativeFocus, String> {
    if !crate::input::completion_enabled() { return Err("Completion is paused".into()); }
    let started = Instant::now();
    if Instant::now() >= request.deadline {
        return Err("Completion expired".into());
    }
    let focus =
        read_focus(system, previous, next_id)?.ok_or("Editable input is no longer focused")?;
    if !focus.snapshot.matches_for_insertion(&request.expected) {
        return Err("Focus, selection or text changed; completion cancelled".into());
    }
    // Chromium can advertise AXSelectedText as writable and return success
    // without changing the DOM value. Use keyboard text input for all controls;
    // never attempt an AX write and then risk duplicating it with a fallback.
    if Instant::now() >= request.deadline || !focus_is_current(system, &focus, request.revision) {
        return Err("Focus changed before insertion".into());
    }
    log::info!(
        "[macos_insert] begin route=Unicode pid={} chars={} utf16_units={}",
        focus.pid,
        request.text.chars().count(),
        request.text.encode_utf16().count(),
    );
    // Send Unicode directly to the original PID. Never replace the clipboard
    // or issue a global Cmd+V that could land in another application.
    let source =
        CGEventSource::new(CGEventSourceStateID::Private).map_err(|_| "No event source")?;
    let until = Instant::now() + Duration::from_secs(2);
    let mut content = focus.snapshot.input_element.content.clone();
    let mut selection = focus.snapshot.selection;
    for chunk in unicode_chunks(&request.text) {
        // Verify each delivered prefix before continuing, so dropped events
        // cannot silently truncate a long completion. Never retry a prefix.
        let next = inserted_text(&content, selection, chunk).ok_or("Invalid selection")?;
        let down = CGEvent::new_keyboard_event(source.clone(), 49, true)
            .map_err(|_| "No keyboard event")?;
        let up = CGEvent::new_keyboard_event(source.clone(), 49, false)
            .map_err(|_| "No keyboard event")?;
        for event in [&down, &up] {
            event.set_flags(CGEventFlags::empty());
            event.set_string(chunk);
            event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, INJECTED_EVENT_TAG);
        }
        let range =
            attr(&focus.element, "AXSelectedTextRange").and_then(|v| value::<CFRange>(&v, 4));
        if Instant::now() >= until
            || string(&focus.element, "AXValue").as_deref() != Some(content.as_str())
            || !range.is_some_and(|r| {
                r.location == selection.location as isize && r.length == selection.length as isize
            })
            || !focus_is_current(system, &focus, request.revision)
        {
            return Err("Input changed or insertion timed out; remaining text cancelled".into());
        }
        down.post_to_pid(focus.pid);
        up.post_to_pid(focus.pid);
        selection.location += chunk.encode_utf16().count();
        selection.length = 0;
        verify_value(
            &focus,
            &next,
            selection,
            until.min(Instant::now() + Duration::from_millis(450)),
        )?;
        content = next;
    }
    // A partial acceptance must bind the remainder to the new caret and value,
    // never to the snapshot from before insertion.
    let updated = read_focus(system, Some(&focus), next_id)?
        .ok_or("Focus changed after insertion; remaining completion cancelled")?;
    let mut expected = focus.snapshot.clone();
    expected.input_element.content = content;
    expected.selection = selection;
    if !updated.snapshot.matches_for_insertion(&expected)
        || !focus_is_current(system, &focus, request.revision)
    {
        return Err("Input changed after insertion; remaining completion cancelled".into());
    }
    log::info!(
        "[macos_insert] verified elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(updated)
}

pub fn collect_input_focus() {
    let (tx, rx) = mpsc::sync_channel::<FillRequest>(1);
    *FILL_SENDER.lock().unwrap() = Some(tx);
    std::thread::Builder::new()
        .name("accessibility".into())
        .spawn(move || {
            let system = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) };
            unsafe {
                AXUIElementSetMessagingTimeout(system.as_CFTypeRef(), AX_TIMEOUT);
            }
            let mut previous = None;
            let mut next_id = 0;
            let mut recovery = FocusReadRecovery::default();
            loop {
                let interval = crate::config::get_config()
                    .map(|c| c.ui_automation.collect_interval)
                    .unwrap_or(100)
                    .max(50);
                match rx.recv_timeout(Duration::from_millis(interval)) {
                    Ok(request) => {
                        let result = insert(&system, previous.as_ref(), &mut next_id, &request)
                            .map(|updated| {
                                let snapshot = updated.snapshot.clone();
                                previous = Some(updated);
                                // Publish before acknowledging: the input listener must
                                // not see the pre-insertion snapshot when selection resumes.
                                *super::CURRENT_FOCUS_INFO.write().unwrap() =
                                    Some(snapshot.clone());
                                FOCUS_AVAILABLE.store(true, Ordering::Release);
                                snapshot
                            });
                        let _ = request.reply.send(result);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if !crate::input::completion_enabled() {
                    FOCUS_AVAILABLE.store(false, Ordering::Release);
                    previous = None;
                    recovery = FocusReadRecovery::default();
                    *super::CURRENT_FOCUS_INFO.write().unwrap() = None;
                    continue;
                }
                match read_focus(&system, previous.as_ref(), &mut next_id) {
                    Ok(next) => {
                        recovery.recovered();
                        let available = next.is_some();
                        previous = next;
                        *super::CURRENT_FOCUS_INFO.write().unwrap() =
                            previous.as_ref().map(|p| p.snapshot.clone());
                        FOCUS_AVAILABLE.store(available, Ordering::Release);
                    }
                    Err(stage) => {
                        // A failed AX IPC is not proof of a blur. Preserve the
                        // same native identity and displayed candidate briefly;
                        // acceptance stays disabled until a fresh read succeeds.
                        FOCUS_AVAILABLE.store(false, Ordering::Release);
                        if recovery.unavailable_since.is_none() && previous.is_some() {
                            log::info!("[macos_focus] AX read unavailable stage={stage}; retaining display for up to 500ms");
                        }
                        if !recovery.retain_last_focus(Instant::now()) {
                            previous = None;
                            *super::CURRENT_FOCUS_INFO.write().unwrap() = None;
                        }
                    }
                }
            }
        })
        .expect("Could not start Accessibility worker");
}

/// Exercise the real acceptance state machine without sending OS events.
#[cfg(test)]
pub(crate) fn mock_insertion_worker(
    mut current: FocusedInput,
    count: usize,
) -> std::thread::JoinHandle<FocusedInput> {
    let (tx, rx) = mpsc::sync_channel::<FillRequest>(1);
    *FILL_SENDER.lock().unwrap() = Some(tx);
    FOCUS_AVAILABLE.store(true, Ordering::Release);
    std::thread::spawn(move || {
        for _ in 0..count {
            let request = rx
                .recv_timeout(Duration::from_secs(2))
                .expect("expected acceptance request");
            assert!(
                current.matches_for_insertion(&request.expected),
                "acceptance used a stale target"
            );
            current.input_element.content = inserted_text(
                &current.input_element.content,
                current.selection,
                &request.text,
            )
            .unwrap();
            current.selection.location += request.text.encode_utf16().count();
            current.selection.length = 0;
            *super::CURRENT_FOCUS_INFO.write().unwrap() = Some(current.clone());
            request.reply.send(Ok(current.clone())).unwrap();
        }
        *FILL_SENDER.lock().unwrap() = None;
        current
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn brief_ax_outages_keep_identity_but_persistent_outages_expire() {
        let mut recovery = FocusReadRecovery::default();
        let now = Instant::now();
        assert!(recovery.retain_last_focus(now));
        assert!(recovery.retain_last_focus(now + Duration::from_millis(100)));
        assert!(recovery.retain_last_focus(now + Duration::from_millis(499)));
        assert!(!recovery.retain_last_focus(now + Duration::from_millis(500)));
        recovery.recovered();
        assert!(recovery.retain_last_focus(now + Duration::from_secs(2)));
    }

    #[test]
    fn unicode_event_payloads_preserve_long_text_and_surrogate_boundaries() {
        for text in [
            "".into(),
            "a".repeat(700),
            "中😀e\u{301}\n".repeat(100),
            format!("{}😀末尾", "a".repeat(19)),
        ] {
            let chunks = unicode_chunks(&text);
            assert_eq!(chunks.concat(), text);
            assert!(chunks
                .iter()
                .all(|chunk| !chunk.is_empty() && chunk.encode_utf16().count() <= 20));
        }
    }
    #[test]
    fn insertion_preserves_unicode_and_text_after_caret() {
        let text = "你好😀world";
        assert_eq!(
            inserted_text(
                text,
                TextRange {
                    location: 4,
                    length: 0
                },
                "！"
            )
            .as_deref(),
            Some("你好😀！world")
        );
        assert_eq!(
            inserted_text(
                text,
                TextRange {
                    location: 2,
                    length: 2
                },
                "🌍"
            )
            .as_deref(),
            Some("你好🌍world")
        );
        assert!(inserted_text(
            text,
            TextRange {
                location: 3,
                length: 0
            },
            "x"
        )
        .is_none());
        assert!(utf16_byte_index(text, 100).is_none());
    }
    #[test]
    fn secure_and_noneditable_controls_are_excluded() {
        assert!(editable_role("AXTextArea", "", false));
        assert!(!editable_role("AXTextField", "AXSecureTextField", false));
        assert!(!editable_role("AXTextField", "", true));
        assert!(!editable_role("AXWebArea", "", false));
        assert!(!editable_role("AXStaticText", "", false));
    }
}

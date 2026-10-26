//! AppKit policy for passive hint windows. Call native functions on the main thread.
use core_foundation::{base::TCFType, string::CFString};
use core_graphics::{
    display::CGDisplay,
    geometry::{CGPoint, CGRect, CGSize},
};
use std::ffi::{c_char, c_void};

#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *const c_void;
}

unsafe fn selector(name: &'static [u8]) -> *const c_void {
    sel_registerName(name.as_ptr().cast())
}

unsafe fn get_object(obj: *mut c_void, name: &'static [u8]) -> *mut c_void {
    let send: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    send(obj, selector(name))
}

unsafe fn send_value<T>(obj: *mut c_void, name: &'static [u8], value: T) {
    let send: unsafe extern "C" fn(*mut c_void, *const c_void, T) =
        std::mem::transmute(objc_msgSend as *const ());
    send(obj, selector(name), value);
}

pub fn set_app_activation_policy_accessory() {
    unsafe {
        let cls = objc_getClass(b"NSApplication\0".as_ptr().cast());
        let app = get_object(cls, b"sharedApplication\0");
        send_value(app, b"setActivationPolicy:\0", 1_isize);
    }
}

pub unsafe fn configure_overlay(window: *mut c_void) {
    const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
    const STATIONARY: usize = 1 << 4;
    const IGNORES_CYCLE: usize = 1 << 6;
    const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
    send_value(
        window,
        b"setCollectionBehavior:\0",
        CAN_JOIN_ALL_SPACES | STATIONARY | IGNORES_CYCLE | FULL_SCREEN_AUXILIARY,
    );
    send_value(window, b"setExcludedFromWindowsMenu:\0", 1_u8);
    send_value(window, b"setIgnoresMouseEvents:\0", 1_u8);

    // IgnoresCycle only affects AppKit's own window cycle. AltTab enumerates
    // AXWindows and admits AXStandardWindow / AXDialog, even for borderless
    // accessory apps. These passive surfaces are floating auxiliary windows.
    let subrole = CFString::new("AXFloatingWindow");
    send_value(
        window,
        b"setAccessibilitySubrole:\0",
        subrole.as_concrete_TypeRef(),
    );

    // Tao's set_always_on_top asynchronously writes level 5, below Dock (20).
    // Use one native owner for the level so a queued setter cannot overwrite it.
    send_value(window, b"setLevel:\0", 25_isize); // NSStatusWindowLevel
}

pub unsafe fn set_overlay_visible(window: *mut c_void, visible: bool) {
    if visible {
        configure_overlay(window);
        // Tauri show() calls makeKeyAndOrderFront on this Tao version. Hints
        // must leave the target app focused, including for Space-at-pointer.
        let send: unsafe extern "C" fn(*mut c_void, *const c_void) =
            std::mem::transmute(objc_msgSend as *const ());
        send(window, selector(b"orderFrontRegardless\0"));
    } else {
        send_value(window, b"orderOut:\0", std::ptr::null::<c_void>());
    }
}

// CGRect is returned indirectly by the x86_64 Objective-C ABI.
unsafe fn get_rect(obj: *mut c_void, name: &'static [u8]) -> CGRect {
    #[cfg(target_arch = "aarch64")]
    {
        let send: unsafe extern "C" fn(*mut c_void, *const c_void) -> CGRect =
            std::mem::transmute(objc_msgSend as *const ());
        send(obj, selector(name))
    }
    #[cfg(target_arch = "x86_64")]
    {
        let mut rect = CGRect::default();
        let send: unsafe extern "C" fn(*mut CGRect, *mut c_void, *const c_void) =
            std::mem::transmute(objc_msgSend_stret as *const ());
        send(&mut rect, obj, selector(name));
        rect
    }
}

fn ax_rect(rect: CGRect, primary_height: f64) -> CGRect {
    CGRect::new(
        &CGPoint::new(
            rect.origin.x,
            primary_height - rect.origin.y - rect.size.height,
        ),
        &rect.size,
    )
}

/// Read on the main thread; exclude the Dock and menu bar, including on screens
/// to the left of or above the primary screen. An offset anchor can be offscreen.
unsafe fn work_area(x: f64, y: f64) -> CGRect {
    let primary = CGDisplay::main().bounds();
    let screens = get_object(objc_getClass(b"NSScreen\0".as_ptr().cast()), b"screens\0");
    let count: unsafe extern "C" fn(*mut c_void, *const c_void) -> usize =
        std::mem::transmute(objc_msgSend as *const ());
    let at: unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    let mut nearest = (f64::INFINITY, primary);
    for i in 0..count(screens, selector(b"count\0")) {
        let screen = at(screens, selector(b"objectAtIndex:\0"), i);
        let frame = ax_rect(get_rect(screen, b"frame\0"), primary.size.height);
        let dx = x - x.clamp(frame.origin.x, frame.origin.x + frame.size.width);
        let dy = y - y.clamp(frame.origin.y, frame.origin.y + frame.size.height);
        let distance = dx * dx + dy * dy;
        if distance < nearest.0 {
            nearest = (
                distance,
                ax_rect(get_rect(screen, b"visibleFrame\0"), primary.size.height),
            );
        }
    }
    nearest.1
}

const MARGIN: f64 = 8.0;
fn limits(bounds: CGRect) -> [f64; 2] {
    [
        (bounds.size.width - 2.0 * MARGIN).clamp(1.0, 560.0),
        (bounds.size.height - 2.0 * MARGIN).clamp(1.0, 32.0),
    ]
}

pub unsafe fn overlay_limits(x: f64, y: f64) -> [f64; 2] {
    limits(work_area(x, y))
}

fn fitted_rect(bounds: CGRect, x: f64, y: f64, width: f64, height: f64) -> CGRect {
    let [max_width, max_height] = limits(bounds);
    let width = width.clamp(1.0, max_width);
    let height = height.clamp(1.0, max_height);
    let left = bounds.origin.x + MARGIN.min((bounds.size.width - width).max(0.0) / 2.0);
    let top = bounds.origin.y + MARGIN.min((bounds.size.height - height).max(0.0) / 2.0);
    let right = (bounds.origin.x + bounds.size.width - MARGIN - width).max(left);
    let bottom = (bounds.origin.y + bounds.size.height - MARGIN - height).max(top);
    let y = if y > bottom { y - height } else { y };
    CGRect::new(
        &CGPoint::new(x.clamp(left, right), y.clamp(top, bottom)),
        &CGSize::new(width, height),
    )
}

/// AX coordinates use the primary display's top-left; AppKit uses bottom-left.
pub unsafe fn resize_and_position(window: *mut c_void, x: f64, y: f64, width: f64, height: f64) {
    let rect = fitted_rect(work_area(x, y), x, y, width, height);
    send_value(window, b"setContentSize:\0", rect.size);
    send_value(
        window,
        b"setFrameTopLeftPoint:\0",
        CGPoint::new(
            rect.origin.x,
            CGDisplay::main().bounds().size.height - rect.origin.y,
        ),
    );
    set_overlay_visible(window, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_overlay_stays_inside_each_work_area() {
        // Primary, left and above monitors, and a narrow display.
        for bounds in [
            CGRect::new(&CGPoint::new(0.0, 25.0), &CGSize::new(1440.0, 800.0)),
            CGRect::new(&CGPoint::new(-1280.0, 25.0), &CGSize::new(1280.0, 700.0)),
            CGRect::new(&CGPoint::new(0.0, -900.0), &CGSize::new(1600.0, 850.0)),
            CGRect::new(&CGPoint::new(0.0, 25.0), &CGSize::new(320.0, 200.0)),
        ] {
            for (x, y) in [
                (bounds.origin.x - 10.0, bounds.origin.y - 28.0),
                (
                    bounds.origin.x + bounds.size.width - 1.0,
                    bounds.origin.y + bounds.size.height - 1.0,
                ),
            ] {
                let rect = fitted_rect(bounds, x, y, 10000.0, 2000.0);
                assert!(rect.origin.x >= bounds.origin.x + MARGIN);
                assert!(rect.origin.y >= bounds.origin.y + MARGIN);
                assert!(
                    rect.origin.x + rect.size.width <= bounds.origin.x + bounds.size.width - MARGIN
                );
                assert!(
                    rect.origin.y + rect.size.height
                        <= bounds.origin.y + bounds.size.height - MARGIN
                );
                assert!(rect.size.width <= 560.0 && rect.size.height <= 32.0);
            }
        }
    }

    #[test]
    fn bottom_anchor_places_hint_above_the_caret() {
        let bounds = CGRect::new(&CGPoint::new(0.0, 25.0), &CGSize::new(1440.0, 800.0));
        let rect = fitted_rect(bounds, 600.0, 790.0, 300.0, 180.0);
        assert_eq!(rect.origin.y + rect.size.height, 790.0);
    }

    #[test]
    fn screen_coordinates_preserve_negative_monitors() {
        let appkit = CGRect::new(&CGPoint::new(-1280.0, 900.0), &CGSize::new(1280.0, 800.0));
        let ax = ax_rect(appkit, 900.0);
        assert_eq!((ax.origin.x, ax.origin.y), (-1280.0, -800.0));
        assert_eq!((ax.size.width, ax.size.height), (1280.0, 800.0));
    }
}

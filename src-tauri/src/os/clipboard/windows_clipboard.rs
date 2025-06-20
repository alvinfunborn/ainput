use std::collections::VecDeque;
use std::sync::Mutex;
use windows::Win32::System::DataExchange::{OpenClipboard, CloseClipboard, GetClipboardData, AddClipboardFormatListener, SetClipboardData, EmptyClipboard};
use windows::Win32::Foundation::{HWND, HANDLE, HGLOBAL};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock, GlobalAlloc, GMEM_MOVEABLE};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::ffi::OsStrExt;
use std::ffi::OsStr;
use once_cell::sync::Lazy;

const PER_ITEM_LIMIT: usize = 1024; // 单条最大字符数
const TOTAL_LIMIT: usize = 1024; // 总历史最大字符数
const CF_UNICODETEXT: u32 = 13; // Win32 剪贴板 Unicode 文本格式

static CLIPBOARD_HISTORY: Lazy<Mutex<ClipboardHistory>> = Lazy::new(|| Mutex::new(ClipboardHistory::new()));

pub struct ClipboardHistory {
    history: VecDeque<String>,
    total_size: usize,
}

impl ClipboardHistory {
    pub fn new() -> Self {
        Self { history: VecDeque::new(), total_size: 0 }
    }
    pub fn add(&mut self, text: String) {
        let mut text = text;
        if text.len() > PER_ITEM_LIMIT {
            text.truncate(PER_ITEM_LIMIT);
        }
        if self.history.back().map_or(false, |last| last == &text) {
            return;
        }
        self.total_size += text.len();
        self.history.push_back(text);
        while self.total_size > TOTAL_LIMIT {
            if let Some(removed) = self.history.pop_front() {
                self.total_size -= removed.len();
            }
        }
    }
    pub fn get_all(&self) -> Vec<String> {
        self.history.iter().cloned().collect()
    }
}

pub fn get_clipboard_text() -> Option<String> {
    unsafe {
        if OpenClipboard(None).is_ok() {
            if let Ok(handle) = GetClipboardData(CF_UNICODETEXT) {
                let ptr = GlobalLock(HGLOBAL(handle.0));
                if !ptr.is_null() {
                    let mut len = 0;
                    while *(ptr as *const u16).add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr as *const u16, len);
                    let text = OsString::from_wide(slice).to_string_lossy().to_string();
                    GlobalUnlock(HGLOBAL(handle.0));
                    CloseClipboard().ok();
                    return Some(text);
                }
            }
            CloseClipboard().ok();
        }
        None
    }
}

// 注册剪贴板变更事件，需在主线程调用
pub fn register_clipboard_listener(hwnd: HWND) -> bool {
    unsafe { AddClipboardFormatListener(hwnd).is_ok() }
}

// 在窗口消息循环中处理WM_CLIPBOARDUPDATE
pub fn handle_clipboard_update() {
    if let Some(text) = get_clipboard_text() {
        let mut hist = CLIPBOARD_HISTORY.lock().unwrap();
        hist.add(text);
    }
}

pub fn get_clipboard_history() -> Vec<String> {
    CLIPBOARD_HISTORY.lock().unwrap().get_all() 
}

pub fn set_clipboard_text(text: &str) -> bool {
    unsafe {
        if OpenClipboard(None).is_ok() {
            EmptyClipboard().ok();
            // 转为 UTF-16
            let wide: Vec<u16> = OsStr::new(text).encode_wide().chain(Some(0)).collect();
            let size = wide.len() * std::mem::size_of::<u16>();
            let hglobal_result = GlobalAlloc(GMEM_MOVEABLE, size);
            if let Ok(hglobal) = hglobal_result {
                let ptr = GlobalLock(hglobal) as *mut u8;
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr, size);
                    GlobalUnlock(hglobal);
                    if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hglobal.0))).is_ok() {
                        CloseClipboard().ok();
                        return true;
                    }
                }
            }
            CloseClipboard().ok();
        }
        false
    }
}

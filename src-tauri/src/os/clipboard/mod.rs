#[cfg(target_os = "windows")]
pub mod windows_clipboard;
#[cfg(target_os = "windows")]
pub use windows_clipboard::get_clipboard_history;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::get_clipboard_history;

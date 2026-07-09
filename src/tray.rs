//! Menu bar status item, backed by the ObjC implementation in
//! `src/native/tray.m` (AppKit `NSStatusItem`).
//!
//! The countdown is shown as plain text in the menu bar; the item's menu has
//! a single "Quit" entry that terminates the process.

use std::ffi::{CString, c_char};

unsafe extern "C" {
    fn tray_run();
    fn tray_set_title(title: *const c_char);
    fn tray_set_status(status: *const c_char);
    fn tray_show_person();
    fn tray_set_log_path(path: *const c_char);
}

/// Creates the status item and runs the AppKit event loop. Never returns:
/// "Quit" terminates the process. Must be called on the main thread.
pub fn run() -> ! {
    unsafe { tray_run() }
    unreachable!("tray_run only returns when the app is terminating")
}

/// Tells the tray where this run's log file lives, so the menu's "View Log"
/// item can open it. Call once at startup; until then the item does nothing.
/// Thread-safe like [`set_title`].
pub fn set_log_path(path: &str) {
    let Ok(path) = CString::new(path) else { return };
    unsafe { tray_set_log_path(path.as_ptr()) }
}

/// Shows a person icon instead of text, indicating the user is on the current
/// call. Cleared by the next [`set_title`]. Thread-safe like [`set_title`].
pub fn show_person() {
    unsafe { tray_show_person() }
}

/// Updates the status line at the top of the tray menu (e.g. "Next: standup
/// at 14:00"). Thread-safe like [`set_title`].
pub fn set_status(status: &str) {
    let Ok(status) = CString::new(status) else { return };
    unsafe { tray_set_status(status.as_ptr()) }
}

/// Sets the menu bar text (e.g. "5", "-2", "..."), replacing a person icon if
/// one is shown. Thread-safe: the update is dispatched to the main queue, and
/// is queued if called before [`run`].
pub fn set_title(title: &str) {
    // Interior NULs can't occur in the countdown strings we generate; skip the
    // update rather than panicking if that ever changes.
    let Ok(title) = CString::new(title) else { return };
    unsafe { tray_set_title(title.as_ptr()) }
}

//! Menu bar status item, backed by the ObjC implementation in
//! `src/native/tray.m` (AppKit `NSStatusItem`).
//!
//! The countdown is shown as plain text in the menu bar; the item's menu has
//! a single "Quit" entry that terminates the process.

use std::ffi::{CString, c_char};

unsafe extern "C" {
    fn tray_run();
    fn tray_set_title(title: *const c_char);
}

/// Creates the status item and runs the AppKit event loop. Never returns:
/// "Quit" terminates the process. Must be called on the main thread.
pub fn run() -> ! {
    unsafe { tray_run() }
    unreachable!("tray_run only returns when the app is terminating")
}

/// Sets the menu bar text (e.g. "5", "-2", "..."). Thread-safe: the update is
/// dispatched to the main queue, and is queued if called before [`run`].
pub fn set_title(title: &str) {
    // Interior NULs can't occur in the countdown strings we generate; skip the
    // update rather than panicking if that ever changes.
    let Ok(title) = CString::new(title) else { return };
    unsafe { tray_set_title(title.as_ptr()) }
}

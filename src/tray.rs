//! Menu bar status item, backed by the ObjC implementation in
//! `src/native/tray.m` (AppKit `NSStatusItem`).
//!
//! The countdown is plain text; the menu has a status line plus "Dismiss"
//! (toggles to "Revert dismiss"), "View Log", "About nextcall" and "Quit".
//! The tray owns the dismiss toggle; Rust polls [`dismissed_ts`] each tick.

use std::ffi::{CString, c_char};

unsafe extern "C" {
    fn tray_run();
    fn tray_set_title(title: *const c_char);
    fn tray_set_status(status: *const c_char);
    fn tray_set_log_path(path: *const c_char);
    fn tray_set_dismiss_target(start_ts: i64);
    fn tray_dismissed_ts() -> i64;
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

/// Updates the status line at the top of the tray menu (e.g. "Next: standup
/// at 14:00"). Thread-safe like [`set_title`].
pub fn set_status(status: &str) {
    let Ok(status) = CString::new(status) else { return };
    unsafe { tray_set_status(status.as_ptr()) }
}

/// Arms the menu's "Dismiss" item with the current call's start unix time
/// (0 disables it: no upcoming call), also expiring a recorded dismissal that
/// no longer refers to that call. Call every tick so a click always targets
/// the call the user is looking at. Thread-safe.
pub fn set_dismiss_target(start_ts: i64) {
    unsafe { tray_set_dismiss_target(start_ts) }
}

/// The start unix time of the call the user dismissed via the menu, or
/// `None`. The tray owns the dismiss toggle; the caller must match this
/// against the *current* next call — a stale value (the call changed while we
/// slept) must be ignored, never applied to a different call. Thread-safe.
pub fn dismissed_ts() -> Option<i64> {
    match unsafe { tray_dismissed_ts() } {
        0 => None,
        ts => Some(ts),
    }
}

/// Sets the menu bar text (e.g. "5", "-2", "..."). Thread-safe: the update is
/// dispatched to the main queue, and is queued if called before [`run`].
pub fn set_title(title: &str) {
    // Interior NULs can't occur in the countdown strings we generate; skip the
    // update rather than panicking if that ever changes.
    let Ok(title) = CString::new(title) else { return };
    unsafe { tray_set_title(title.as_ptr()) }
}

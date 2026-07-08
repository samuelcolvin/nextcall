//! System notifications, backed by the ObjC implementation in
//! `src/native/notifications.m` (macOS UserNotifications framework).
//!
//! This module is a thin C FFI wrapper: only UTF-8 C strings cross the
//! boundary. Foot-gun: notifications require running from a signed `.app`
//! bundle with a `CFBundleIdentifier` - they do nothing from a bare binary.

use std::ffi::{CString, c_char};
use std::ptr;

unsafe extern "C" {
    fn notifications_startup();
    fn notifications_send(title: *const c_char, subtitle: *const c_char, body: *const c_char, url: *const c_char);
}

/// Converts a Rust string for the C boundary, stripping interior NUL bytes
/// (which are impossible in real calendar data but must not cause a panic).
fn cstring(s: &str) -> CString {
    CString::new(s).unwrap_or_else(|_| CString::new(s.replace('\0', "")).expect("NUL bytes were just removed"))
}

/// Installs the notification delegate, requests permission, and registers the
/// "Join" action category. Must be called once at startup, before [`send`].
pub fn startup() {
    unsafe { notifications_startup() }
}

/// Sends a system notification immediately (with the "Blow" sound and active
/// interruption level). If `url` is given, the notification gets a "Join"
/// button and any click on it opens the link. Safe to call from any thread.
pub fn send(title: &str, subtitle: Option<&str>, body: &str, url: Option<&str>) {
    let title = cstring(title);
    let subtitle = subtitle.map(cstring);
    let body = cstring(body);
    let url = url.map(cstring);
    unsafe {
        notifications_send(
            title.as_ptr(),
            subtitle.as_ref().map_or(ptr::null(), |s| s.as_ptr()),
            body.as_ptr(),
            url.as_ref().map_or(ptr::null(), |s| s.as_ptr()),
        )
    }
}

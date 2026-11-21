//! Call-activity detection: is the user already on a call? Backed by
//! `src/native/camera.m` (CoreMediaIO) and `src/native/microphone.m` (CoreAudio).
//!
//! Used to skip nags/speech when the user has joined, including audio-only calls.

unsafe extern "C" {
    fn camera_is_active() -> bool;
    fn microphone_is_active() -> bool;
}

/// Returns true if any camera or microphone is in use by another process, i.e.
/// the user is likely on a call. Enumerates devices/processes, so avoid hot loops.
/// Foot-gun: any capture counts (dictation, voice memos), not just call apps.
pub fn in_call() -> bool {
    // camera first: the stronger signal, and short-circuits the mic check
    unsafe { camera_is_active() || microphone_is_active() }
}

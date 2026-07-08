//! Camera-activity detection, backed by the implementation in
//! `src/native/camera.m` (CoreMediaIO hardware API).
//!
//! Used to skip notifications/speech when the user is already on a call.

unsafe extern "C" {
    fn camera_is_active() -> bool;
}

/// Returns true if any video device is in use by some process, i.e. the user
/// is likely on a call. Involves a device enumeration, so avoid hot loops.
pub fn camera_active() -> bool {
    unsafe { camera_is_active() }
}

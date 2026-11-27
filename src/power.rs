//! System sleep tracking, backed by `src/native/power.m` (`NSWorkspace`
//! sleep/wake notifications).
//!
//! Used to keep speech silent while the laptop sleeps: macOS still runs the
//! process during dark wakes (Power Nap), which would otherwise fire alerts
//! with the lid closed.

unsafe extern "C" {
    fn power_start();
    fn power_asleep() -> bool;
}

/// Registers the sleep/wake observers. Call once at startup; they're delivered
/// via the main run loop, so they only take effect once `tray::run` is running.
pub fn start() {
    unsafe { power_start() }
}

/// True from the moment the system starts going to sleep until the next full
/// (user) wake — including dark wakes in between, when our code still runs.
pub fn asleep() -> bool {
    unsafe { power_asleep() }
}

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Nextcall is a macOS menu bar application written in Rust that monitors your calendar for upcoming video calls and provides timely notifications. It displays a countdown in the system tray and sends alerts when meetings are about to start or are already in progress. Notifications use both system Notifications and audio announcement.

### Docstrings and comments.

IMPORTANT: every struct, enum and function should have an concise docstring to
explain what it does and why; and any considerations or potential foot-guns of using that type.

The only exception is trait implementation methods where a docstring is not necessary if the method is self-explanatory.

It's important that docstrings cover the motivation and primary usage patterns of code, not just the simple "what it does".

Similarly, you should add comments to code, especially if the code is complex or esoteric.

Comments and field docstrings should almost never be more than 3 lines, mostly 1 line. Function and struct docstrings should be concise, generally <= 5 lines.

Only add examples to docstrings of public functions and structs, examples should be <=8 lines, if the example is more, remove it.

If you add example code to docstrings, it must be run in tests. NEVER add examples that are ignored.

If you encounter a comment or docstring that's out of date - you MUST update it to be correct.

Similarly, if you encounter code that has no docstrings or comments, or they are minimal, you should add more detail.

Always use single back-ticks in python docstrings - they should be markdown, not rst!

NOTE: COMMENTS AND DOCSTRINGS ARE EXTREMELY IMPORTANT TO THE LONG TERM HEALTH OF THE PROJECT.

NOTE: COMMENTS AND DOCSTRINGS SHOULD BE CONCISE - EXCESSIVELY VERBOSE DOCSTRINGS MAKE THE CODE HARDER TO READ AND MAINTAIN!


## Configuration

The app requires a `nextcall.toml` configuration file, which should be placed either in:
- The current working directory, or
- The home directory (`~/nextcall.toml`)

Configuration format:
```toml
ical_url = "https://your-calendar-ics-url"
eleven_labs_key = "your-api-key"  # Optional, for TTS via ElevenLabs
```

## Building and Check

```bash
# Build the development version
cargo build

# Format code
make format

# Lint code
make lint
```

**ALWASY RUN `make lint` AT EVERY STAGE TO ENSURE CODE QUALITY**

## Architecture

All macOS interaction is implemented in Objective-C (`src/native/*.m`), exposed to Rust as plain C functions and compiled into the cargo build by `build.rs` via the `cc` crate. Only C types (UTF-8 strings, bools) cross the boundary — see `rust-objc.md` for the pattern. Rust modules (`notifications.rs`, `camera.rs`, `tray.rs`) are thin FFI wrappers.

### Main Entry Point (`src/main.rs`)
The main thread runs the AppKit event loop (`tray::run`, never returns; the "Quit" menu item terminates the process). Before that it loads config, registers for notifications, and spawns the background loop, which is almost stateless — its only state is the `CalendarFeed` cache and the previous tick's timestamp:
1. Ask the feed for the calendar (cached; network at most once per TTL), ~10s before the scheduled tick so fetch latency never delays an alert
2. Read the tray's dismiss toggle and the camera state, then let the pure `logic::step(cal, now, prev_tick, camera_active, dismissed)` decide display, status, alert and sleep
3. Apply the side effects (tray, notification + speech) and sleep until the next tick (wall-clock deadlines, so system sleep and blocking speech don't skew the schedule)

### Core Business Logic (`src/logic.rs`)
`step()` is a pure function of `(cal, now, prev_tick, camera_active, dismissed)` — no clock, no IO — returning what the tray shows, the menu status line, an alert if one is due, and how long to sleep. Key rules:
- **Alerts are boundary crossings**: alert instants are start + k minutes (k = 0..10); one fires iff it lies in `(prev_tick, now]` — exactly-once by construction, no dedup state. Nags (k ≥ 1) are suppressed once the camera is active; the start alert always notifies (speech stays camera-gated)
- **Dismiss**: the tray menu's "Dismiss" item mutes all alerts for the current call, and flips to "Revert dismiss" to undo. The tray owns the toggle (and renders it instantly); like the camera, the main loop just polls it each tick (`tray::dismissed_ts`) and passes the start time to `step` as `dismissed` only if it matches `next_call` — so a stale dismissal (the call changed while the loop slept) can never mute a different call. Arming the item each tick (`tray_set_dismiss_target` with `next_call`'s start unix time; 0 disables it) also expires a stale dismissal
- Display: a positive countdown to an upcoming call (≤1h away), the negative minutes since it started, or "..." (dismissed-state rendering — the monochrome bell.slash SF Symbol — is owned by the tray, not `step`)
- Sleep: min of next alert instant, event start, start − 1h, and top-of-minute during a countdown; capped at 180s, floored at 1s
- `fire_alert` (side-effectful, called by main) sends the notification and camera-gated speech

### Calendar Integration (`src/ical.rs`)
- Single public type: `CalendarFeed` owns the URL and an internal cache of expanded occurrences; `feed.fetch(now)` refreshes the cache if expired (returning any error; stale data is kept on failure) and `feed.cal(now)` is the pure per-tick window selection returning `Cal { next_call }`
- Cache TTL is dynamic: 60s when `next_call` is within 10 minutes (catches last-minute moves/cancellations), 180s otherwise
- `Cal.next_call` = earliest upcoming event or one started <10 min ago (drives countdown, status and alerts)
- Expands recurring events (RRULE) via the `rrule` crate, honouring EXDATE, override instances (RECURRENCE-ID) and STATUS:CANCELLED
- Extracts video links from URL, LOCATION, or DESCRIPTION fields (Zoom, Google Meet, Microsoft Teams)

### Notifications (`src/notifications.rs` + `src/native/notifications.m`)
macOS UserNotifications framework integration in Objective-C:
- `NCNotificationDelegate` handles notification interactions; clicks (including the "Join" button) open the video link via `NSWorkspace`
- Uses "Blow.aiff" system sound with active interruption level
- Requires notification permissions on first run, and only works from a signed `.app` bundle with a `CFBundleIdentifier`

### Camera Detection (`src/camera.rs` + `src/native/camera.m`)
Uses the CoreMediaIO hardware C API to detect if the camera is active:
- Enumerates CMIO devices and checks `kCMIODevicePropertyDeviceIsRunningSomewhere` on each
- Prevents notifications from interrupting if camera is already in use

### Text-to-Speech (`src/say.rs`)
`tts_friendly` rewrites calendar shorthand in the spoken summary so TTS pronounces it sensibly ("1:1" → "1 to 1", "<>" / name slashes → "and", "w/" → "with", "|" → a pause); notifications and the tray keep the literal title. Dual implementation:
- ElevenLabs API (if `eleven_labs_key` is configured) - uses `rodio` for audio playback
- macOS built-in `say` command with "Moira" voice as fallback

### Tray Icon (`src/tray.rs` + `src/native/tray.m`)
An AppKit `NSStatusItem` whose title is the countdown text — the only images are template glyphs: the stopwatch-lens logo while idle (the "..." title, loaded from `tray-icon.png` in Resources) and the SF Symbol bell while dismissed. A single `render()` derives the display (title, bell icon, Dismiss/Revert item title) from the last title and the dismiss toggle, which the tray owns; `tray_set_dismiss_target`/`tray_dismissed_ts` are the atomics Rust arms/polls each tick. `tray_set_title`/`tray_set_status`/`tray_set_log_path` are thread-safe (dispatch to the main queue); `tray_run` runs the `NSApplication` event loop on the main thread and never returns.

### Build Configuration (`build.rs`)
Compiles `src/native/*.m` with the `cc` crate (ARC enabled) and links the required frameworks: Foundation, AppKit, UserNotifications, CoreMediaIO.

### Icons (`assets/`)
`logo.svg` is the monotone stopwatch-lens logo (black); `logo-white.svg` is the same glyph in white, used as the README's dark-mode `<picture>` source; `appicon.svg` is the glyph in white on a dark plate. `assets/make-icons.sh` regenerates the checked-in artifacts (`AppIcon.icns`, `tray-icon.png`) with `sips` + `iconutil` — rerun it whenever the SVGs change. `build.sh` copies the artifacts into `Contents/Resources`.

## Platform-Specific Notes

- **macOS only**: This application uses macOS-specific frameworks (UserNotifications, AVFoundation, CoreMediaIO)
- Requires macOS notification permissions to function properly
- Uses native `say` command for text-to-speech fallback
- Edition 2024 is specified in Cargo.toml (note: this may require a recent Rust version)

## Development Patterns

- OS interaction lives in Objective-C (`src/native/*.m`); Rust calls it through a small C API (`unsafe extern "C"` declarations in the wrapper modules). Keep the boundary C-only: no ObjC types, no Rust types, just UTF-8 strings and scalars
- Error handling uses `anyhow::Result` for application errors
- Custom error enum (`CalendarError`) for iCal-specific errors

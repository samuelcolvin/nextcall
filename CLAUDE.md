# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

NextCall is a macOS menu bar application written in Rust that monitors your calendar for upcoming video calls and provides timely notifications. It displays a countdown in the system tray and sends alerts when meetings are about to start or are already in progress. Notifications use both system Notifications and audio announcement.

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
The main thread runs the AppKit event loop (`tray::run`, never returns; the "Quit" menu item terminates the process). Before that it:
- Loads config and registers for notifications
- Spawns a background thread that polls the calendar, updates the tray title (thread-safe, dispatched to the main queue in ObjC), and self-paces its sleeps based on how far away the next event is

### Core Business Logic (`src/logic.rs`)
Drives the background loop:
1. `find_events` fetches/parses the iCal feed and keeps the previous events on transient errors
2. `calc_sleep` picks the tray text — "..." when >60 minutes away, minutes countdown when closer — and how long to sleep before re-checking
3. `event_started` runs the alert sequence: notifications at event start, +2 and +5 minutes, with a negative minutes countdown in the tray
4. Camera status is checked before alerts to avoid interrupting active calls, and joining a call cancels the remaining alerts within seconds
5. While any event is in progress, `watch_camera_until` replaces plain sleeps: it polls the camera every 5s and shows the person icon while the user is on the call

### Calendar Integration (`src/ical.rs`)
- Downloads and parses iCal feeds using the `ical` crate
- Expands recurring events (RRULE) via the `rrule` crate, honouring EXDATE, override instances (RECURRENCE-ID) and STATUS:CANCELLED
- Extracts video links from URL, LOCATION, or DESCRIPTION fields
- Supports Zoom, Google Meet, and Microsoft Teams URLs
- Returns a `CalendarEvents` pair: `next` (earliest upcoming event, or one started <10 min ago — drives countdown and alerts) and `in_progress` (most recently started event within the last hour — drives the person icon)

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
Dual implementation:
- ElevenLabs API (if `eleven_labs_key` is configured) - uses `rodio` for audio playback
- macOS built-in `say` command with "Moira" voice as fallback

### Tray Icon (`src/tray.rs` + `src/native/tray.m`)
An AppKit `NSStatusItem` whose title is the countdown text — no image rendering involved. While the user is on the current call (camera active after the alert sequence), a person SF Symbol is shown instead (`tray_show_person`). `tray_set_title`/`tray_show_person` are thread-safe (dispatch to the main queue); `tray_run` runs the `NSApplication` event loop on the main thread and never returns.

### Build Configuration (`build.rs`)
Compiles `src/native/*.m` with the `cc` crate (ARC enabled) and links the required frameworks: Foundation, AppKit, UserNotifications, CoreMediaIO.

## Platform-Specific Notes

- **macOS only**: This application uses macOS-specific frameworks (UserNotifications, AVFoundation, CoreMediaIO)
- Requires macOS notification permissions to function properly
- Uses native `say` command for text-to-speech fallback
- Edition 2024 is specified in Cargo.toml (note: this may require a recent Rust version)

## Development Patterns

- OS interaction lives in Objective-C (`src/native/*.m`); Rust calls it through a small C API (`unsafe extern "C"` declarations in the wrapper modules). Keep the boundary C-only: no ObjC types, no Rust types, just UTF-8 strings and scalars
- Error handling uses `anyhow::Result` for application errors
- Custom error enum (`CalendarError`) for iCal-specific errors

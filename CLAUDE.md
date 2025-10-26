# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

NextCall is a macOS menu bar application written in Rust that monitors your calendar for upcoming video calls and provides timely notifications. It displays a countdown in the system tray and sends alerts when meetings are about to start or are already in progress. Notifications use both system Notifications and audio announcement.

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

**ALWASY RUN `make lint` AND `cargo build` AT EVERY STAGE TO ENSURE CODE QUALITY**

## Architecture

### Main Event Loop (`src/main.rs`)
The application uses `winit` for event loop management and `tray-icon` for the menu bar icon. It runs a non-blocking architecture where:
- Every 10 seconds, the main loop spawns a background thread to check for calendar updates
- Icon updates and event state are communicated back to the main thread via channels (`mpsc`)
- This prevents the UI from freezing during network operations

### Core Business Logic (`src/logic.rs`)
The `step()` function is the heart of the application, called every 10 seconds:
1. Fetches and parses the iCal feed
2. Filters events with video links that are within the alert window (-10 to +∞ minutes from start)
3. Updates the tray icon based on time until next meeting:
   - "..." for events more than 60 minutes away or no events
   - Minutes countdown (e.g., "5", "15") for events within 60 minutes
   - "0" (enlarged) when event has started
4. Manages notification state through `ActiveEvent` struct to prevent duplicate alerts
5. Sends notifications at: event start, 2 minutes after start, and 5 minutes after start
6. Checks camera status before sending notifications to avoid interrupting active calls

### Calendar Integration (`src/ical.rs`)
- Downloads and parses iCal feeds using the `ical` crate
- Extracts video links from URL, LOCATION, or DESCRIPTION fields
- Supports Zoom, Google Meet, and Microsoft Teams URLs
- Returns events sorted by start time

### Notifications (`src/notifications.rs`)
Low-level macOS UserNotifications framework integration using `objc2`:
- Implements a custom delegate (`NotificationDelegate`) to handle notification interactions
- Supports clickable "Join" buttons on notifications that open video links
- Uses "Blow.aiff" system sound with active interruption level
- Requires notification permissions on first run

### Camera Detection (`src/camera.rs`)
Uses macOS AVFoundation and CoreMediaIO frameworks to detect if the camera is active:
- Checks `kCMIODevicePropertyDeviceIsRunningSomewhere` property on all video devices
- Prevents notifications from interrupting if camera is already in use

### Text-to-Speech (`src/say.rs`)
Dual implementation:
- ElevenLabs API (if `eleven_labs_key` is configured) - uses `rodio` for audio playback
- macOS built-in `say` command with "Moira" voice as fallback

### Icon Generation (`src/icon.rs`)
Dynamically generates tray icons with embedded text using `image` and `imageproc` crates. Uses DejaVu Sans font included in `assets/`.

### Build Configuration (`build.rs`)
Links AVFoundation framework required for camera detection.

## Platform-Specific Notes

- **macOS only**: This application uses macOS-specific frameworks (UserNotifications, AVFoundation, CoreMediaIO)
- Requires macOS notification permissions to function properly
- Uses native `say` command for text-to-speech fallback
- Edition 2024 is specified in Cargo.toml (note: this may require a recent Rust version)

## Development Patterns

- Heavy use of `unsafe` code for Objective-C interop via `objc2` crate
- Error handling uses `anyhow::Result` for application errors
- Custom error enum (`CalendarError`) for iCal-specific errors
- Notification state tracking prevents alert spam using `ActiveEvent` with per-event flags

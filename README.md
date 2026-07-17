<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-white.svg">
    <source media="(prefers-color-scheme: light)" srcset="assets/logo.svg">
    <img alt="Nextcall logo" src="assets/logo.svg" width="140">
  </picture>
</div>

# Nextcall

A macOS menu bar app that makes sure you're never late to a video call.

Nextcall watches your calendar's iCal feed and shows a countdown to your next
call in the menu bar.

When a call starts it notifies you with a system notification and a spoken announcement.

## Install

```bash
git clone https://github.com/samuelcolvin/nextcall
cd nextcall
make install          # builds Nextcall.app and copies it to /Applications
open /Applications/Nextcall.app
```

Requires macOS 12+ and a Rust toolchain to build. The app is ad-hoc
code-signed by the build; a signed `.app` bundle is required for macOS to
deliver its notifications (grant permission on first run).

## Configuration

Create `~/nextcall.toml` (the current working directory is also checked):

```toml
# Secret address of your calendar in iCal format, e.g. from Google Calendar:
# Settings > your calendar > Integrate calendar > Secret address in iCal format
ical_url = "https://calendar.google.com/calendar/ical/.../basic.ics"

# Optional: nicer spoken announcements via ElevenLabs text-to-speech
eleven_labs_key = "..."
```

# NextCall

A macOS menu bar app that makes sure you're never late to a video call.

NextCall watches your calendar's iCal feed and shows a countdown to your next
call in the menu bar. When a call starts it notifies you — with a system
notification and a spoken announcement — and keeps nagging every minute until
it sees you've actually joined (your camera is on) or ten minutes have passed.

## What it does

- **Menu bar countdown**: minutes until the next call once it's within an
  hour, negative minutes once it has started, "..." otherwise. The menu shows
  the next/current call's name and time.
- **Alerts that don't give up**: a notification at the start of the call and
  every minute after, for up to ten minutes. Clicking the notification (or its
  "Join" button) opens the video link directly.
- **Spoken announcements**: via the [ElevenLabs](https://elevenlabs.io) API if
  a key is configured, falling back to the built-in macOS `say` command.
- **Camera detection**: once your camera is active the nagging stops, and
  speech is suppressed so announcements never talk over a call you're already
  on.
- **Real-world calendars**: recurring events (RRULE) with exceptions, moved
  instances and cancellations are handled; video links (Google Meet, Zoom,
  Microsoft Teams) are extracted from the URL, location or description fields.

## Install

```bash
git clone https://github.com/samuelcolvin/nextcall
cd nextcall
make install          # builds Nextcall.app and copies it to /Applications
open /Applications/Nextcall.app
```

Use `make install PREFIX=~/Applications` to install elsewhere, or `make run`
to build and run in the foreground with logs in the terminal.

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

## Troubleshooting

Each run writes a log to a temp file — open it via the menu bar item's
"View Log" entry. No notifications usually means notification permission was
denied: check System Settings → Notifications → Nextcall.

## License

[MIT](LICENSE)

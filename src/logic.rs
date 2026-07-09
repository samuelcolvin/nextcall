//! The pure per-tick decision logic: given the calendar state, the current
//! time, the previous tick's time and camera state, [`step`] decides what the
//! tray shows, whether an alert is due, and how long to sleep. Keeping it
//! pure (no clock, no IO) makes the whole alerting behaviour unit-testable.

use crate::ical::{Cal, NEXT_MAX_AGE_MINUTES, NextEvent};
use crate::{notifications, say};
use chrono::{DateTime, TimeDelta, Timelike, Utc};
use std::{borrow::Cow, time::Duration};
use tracing::info;

/// Idle sleep cap: how long the loop may sleep with nothing coming up.
pub const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_secs(180);

/// Tick cadence while a call is in progress, so the tray person icon reflects
/// joining/leaving the call within seconds.
const CAMERA_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// What the tray should show this tick.
#[derive(Debug, PartialEq)]
pub enum TrayIcon {
    /// Countdown text: minutes until the next call ("5") or since its start ("-2").
    Text(String),
    /// Person icon: the user is on a call (camera active during one).
    Person,
    /// Nothing within the countdown hour ("...").
    Idle,
}

impl TrayIcon {
    pub fn show(&self) {
        match self {
            Self::Person => super::tray::show_person(),
            Self::Text(text) => super::tray::set_title(&text),
            Self::Idle => super::tray::set_title("..."),
        }
    }
}

/// The outcome of one [`step`]: everything the main loop must apply.
#[derive(Debug)]
pub struct Step {
    pub icon: TrayIcon,
    /// Text for the status line at the top of the tray menu.
    pub status: String,
    /// An alert due this tick: the event and whole minutes since its start.
    pub alert: Option<(NextEvent, i64)>,
    /// How long to sleep until the next tick.
    pub sleep: Duration,
}

/// Pure per-tick decision. `prev_tick` is the previous invocation's `now`;
/// an alert fires iff its scheduled instant lies in `(prev_tick, now]` -
/// every instant belongs to exactly one tick, so alerts fire exactly once
/// without any dedup state.
pub fn step(cal: &Cal, now: DateTime<Utc>, prev_tick: DateTime<Utc>, camera_active: bool) -> Step {
    Step {
        icon: tray_icon(cal, now, camera_active),
        status: status_line(cal, now),
        alert: pending_alert(cal, now, prev_tick, camera_active),
        sleep: sleep_duration(cal, now),
    }
}

/// Sends the notification (and camera-gated speech) for an alert produced by
/// [`step`]. `minutes` is whole minutes since the event started. Not part of
/// `step` so the decision stays pure; may block for seconds while speaking.
pub fn fire_alert(event: &NextEvent, minutes: i64, camera_active: bool, eleven_labs_key: Option<&str>) {
    info!(
        "alerting for {:?}, {minutes} minutes after start, camera active: {camera_active}",
        event.summary
    );
    let started_description: Cow<'static, str> = match minutes {
        0 => "has started".into(),
        2 => "started two minutes ago, join it now!".into(),
        _ => format!("started {minutes} minutes ago, join it now!").into(),
    };
    notifications::send(
        "Nextcall",
        Some(&format!("Call {started_description}")),
        &event.summary,
        Some(&event.video_link),
    );
    if !camera_active {
        let message = format!("Your call {:?} {}", sayevent_summary(event), started_description);
        let _ = say::say(&message, eleven_labs_key);
    }
}

/// The alert whose scheduled instant (start + k minutes, k = 0..10) lies in
/// `(prev_tick, now]`, if any. Only the latest such instant fires (a tick
/// covering several missed instants alerts once); nags after the start alert
/// stop once the user is on the call.
fn pending_alert(
    cal: &Cal,
    now: DateTime<Utc>,
    prev_tick: DateTime<Utc>,
    camera_active: bool,
) -> Option<(NextEvent, i64)> {
    let event = cal.next_call.as_ref()?;
    // whole minutes since start = k of the latest alert instant at or before now
    let minutes = now.signed_duration_since(event.start_time).num_minutes();
    if !(0..NEXT_MAX_AGE_MINUTES).contains(&minutes) {
        return None;
    }
    let instant = event.start_time + TimeDelta::minutes(minutes);
    if instant <= prev_tick {
        // a previous tick already covered this instant
        return None;
    }
    if minutes >= 1 && camera_active {
        // user is on the call: stop nagging (the start alert always notifies)
        return None;
    }
    Some((event.clone(), minutes))
}

/// The tray icon content. A positive countdown to an upcoming call always
/// wins - it's the one thing worth seeing while busy. The person icon only
/// replaces what refers to the call the user is already on: the negative
/// countdown or the idle "...".
fn tray_icon(cal: &Cal, now: DateTime<Utc>, camera_active: bool) -> TrayIcon {
    let until_start = cal
        .next_call
        .as_ref()
        .map(|event| event.start_time.signed_duration_since(now));

    // minutes rounded down; only shown within an hour of the start
    if let Some(until) = until_start
        && until >= TimeDelta::zero()
        && until <= TimeDelta::hours(1)
    {
        let minutes_until = (until.as_seconds_f32() / 60.0).floor() as i32;
        return TrayIcon::Text(minutes_until.to_string());
    }
    if camera_active && cal.in_call {
        return TrayIcon::Person;
    }
    match until_start {
        // negative countdown; rounds rather than floors (e.g. -2.6 -> "-3")
        Some(until) if until < TimeDelta::zero() => TrayIcon::Text(format!("{:.0}", until.as_seconds_f32() / 60.0)),
        _ => TrayIcon::Idle,
    }
}

/// One-line summary of the calendar state, shown at the top of the tray menu.
fn status_line(cal: &Cal, now: DateTime<Utc>) -> String {
    let local_start = |e: &NextEvent| e.start_time.with_timezone(&chrono::Local).format("%H:%M");
    match cal.next_call {
        Some(ref event) if event.start_time <= now => {
            format!("In progress: {} (started {})", event.summary, local_start(event))
        }
        Some(ref event) => format!("Next: {} at {}", event.summary, local_start(event)),
        None => "No upcoming calls".to_string(),
    }
}

/// How long to sleep until the next instant the loop must act on: the next
/// alert instant, the start (or start - 1h, when the countdown appears), the
/// next top-of-minute countdown tick, or the camera poll while on a call.
/// No post-alert adjustment is needed: an instant already covered by
/// `(prev_tick, now]` can never fire again.
fn sleep_duration(cal: &Cal, now: DateTime<Utc>) -> Duration {
    let mut sleep = DEFAULT_CHECK_INTERVAL;
    if cal.in_call {
        sleep = sleep.min(CAMERA_POLL_INTERVAL);
    }
    if let Some(ref event) = cal.next_call {
        let until_start = event.start_time.signed_duration_since(now);
        if until_start > TimeDelta::zero() {
            // upcoming: wake at start, at start - 1h ("..." -> countdown), and
            // at each top-of-minute while the countdown is showing
            let mut until = until_start;
            let hour_before = until_start - TimeDelta::hours(1);
            if hour_before > TimeDelta::zero() {
                until = until.min(hour_before);
            } else {
                let top_of_minute = TimeDelta::seconds(i64::from(60 - now.second()));
                until = until.min(top_of_minute);
            }
            sleep = sleep.min(until.to_std().unwrap_or(Duration::ZERO));
        } else {
            // started: wake at the next minute boundary from start (the next
            // alert instant, which is also when the negative countdown ticks)
            let minutes = (-until_start).num_minutes();
            let next_instant = event.start_time + TimeDelta::minutes(minutes + 1);
            let until = next_instant.signed_duration_since(now);
            sleep = sleep.min(until.to_std().unwrap_or(Duration::ZERO));
        }
    }
    // the floor is load-bearing: waking a hair before a boundary must cost one
    // extra 1s tick, not a busy-loop
    sleep.max(Duration::from_secs(1))
}

/// Left strip `call` and `-` from the event summary
fn sayevent_summary(event: &NextEvent) -> &str {
    let mut summary = event.summary.as_str().trim_start();
    summary = istrip(summary, "call").trim_start();
    summary = istrip(summary, "-").trim_start();
    summary = istrip(summary, ":").trim_start();
    summary
}

fn istrip<'a>(s: &'a str, prefix: &str) -> &'a str {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        &s[prefix.len()..]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Pinned reference time: 09:08:00 UTC.
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 9, 9, 8, 0).unwrap()
    }

    /// An event starting `minutes_from_now` relative to [`now`].
    fn event(minutes_from_now: i64) -> NextEvent {
        NextEvent {
            start_time: now() + TimeDelta::minutes(minutes_from_now),
            summary: "standup".to_string(),
            video_link: "https://meet.google.com/abc".to_string(),
        }
    }

    fn cal(minutes_from_now: i64, in_call: bool) -> Cal {
        Cal {
            in_call,
            next_call: Some(event(minutes_from_now)),
        }
    }

    fn secs(s: i64) -> TimeDelta {
        TimeDelta::seconds(s)
    }

    #[test]
    fn alert_fires_when_instant_crossed() {
        // event started exactly at `now`; prev tick 5s earlier
        let step = step(&cal(0, true), now(), now() - secs(5), false);
        assert_eq!(step.alert.as_ref().unwrap().1, 0);
    }

    #[test]
    fn alert_does_not_refire_on_next_tick() {
        // prev tick landed exactly on the instant: (prev, now] excludes it
        let start = now() - secs(5);
        let c = Cal {
            in_call: true,
            next_call: Some(NextEvent {
                start_time: start,
                ..event(0)
            }),
        };
        let step = step(&c, now(), start, false);
        assert!(step.alert.is_none());
    }

    #[test]
    fn late_tick_still_fires_crossed_instant() {
        // say-blocked: tick arrives 20s after the +1 minute instant
        let step = step(&cal(-1, true), now() + secs(20), now() - secs(45), false);
        assert_eq!(step.alert.as_ref().unwrap().1, 1);
    }

    #[test]
    fn multiple_crossed_instants_fire_once_with_latest() {
        // a huge gap (laptop asleep) covering the +0..+3 instants: only +3 fires
        let step = step(&cal(-3, true), now(), now() - TimeDelta::minutes(10), false);
        assert_eq!(step.alert.as_ref().unwrap().1, 3);
    }

    #[test]
    fn restart_fires_nothing_then_next_minute() {
        // startup at +2:30: prev_tick == now, nothing fires...
        let step_at_start = step(&cal(-2, true), now() + secs(30), now() + secs(30), false);
        assert!(step_at_start.alert.is_none());
        // ...but the +3 instant fires on the tick that crosses it
        let step_next = step(&cal(-2, true), now() + secs(62), now() + secs(30), false);
        assert_eq!(step_next.alert.as_ref().unwrap().1, 3);
    }

    #[test]
    fn camera_suppresses_nags_but_not_start() {
        let fired = step(&cal(0, true), now(), now() - secs(5), true);
        assert_eq!(fired.alert.as_ref().unwrap().1, 0, "start alert fires despite camera");
        let nag = step(&cal(-2, true), now(), now() - secs(5), true);
        assert!(nag.alert.is_none(), "nag suppressed while on the call");
    }

    #[test]
    fn no_alerts_after_window() {
        // +10 minutes: outside the 0..10 alert window
        let step = step(&cal(-10, true), now(), now() - secs(5), false);
        assert!(step.alert.is_none());
    }

    #[test]
    fn display_states() {
        assert_eq!(step(&Cal::default(), now(), now(), false).icon, TrayIcon::Idle);
        assert_eq!(step(&cal(90, false), now(), now(), false).icon, TrayIcon::Idle);
        // floor: 59m30s away shows "59"
        let c = Cal {
            in_call: false,
            next_call: Some(NextEvent {
                start_time: now() + secs(59 * 60 + 30),
                ..event(0)
            }),
        };
        assert_eq!(step(&c, now(), now(), false).icon, TrayIcon::Text("59".into()));
        // negative countdown rounds: 2m36s ago shows "-3"
        let c = Cal {
            in_call: true,
            next_call: Some(NextEvent {
                start_time: now() - secs(156),
                ..event(0)
            }),
        };
        assert_eq!(step(&c, now(), now(), false).icon, TrayIcon::Text("-3".into()));
        // person only when camera AND in_call
        assert_eq!(step(&cal(-2, true), now(), now(), true).icon, TrayIcon::Person);
        assert_eq!(
            step(
                &Cal {
                    in_call: false,
                    next_call: None
                },
                now(),
                now(),
                true
            )
            .icon,
            TrayIcon::Idle
        );
        // a positive countdown to the next call beats the person icon...
        assert_eq!(
            step(&cal(30, true), now(), now(), true).icon,
            TrayIcon::Text("30".into())
        );
        // ...but the person icon beats the idle "..." (next call >1h away)
        assert_eq!(step(&cal(90, true), now(), now(), true).icon, TrayIcon::Person);
        assert_eq!(
            step(
                &Cal {
                    in_call: true,
                    next_call: None
                },
                now(),
                now(),
                true
            )
            .icon,
            TrayIcon::Person
        );
    }

    #[test]
    fn sleep_durations() {
        // nothing upcoming: idle cap
        assert_eq!(step(&Cal::default(), now(), now(), false).sleep, DEFAULT_CHECK_INTERVAL);
        // 90s to start: countdown showing, wake at the next top-of-minute
        let c = Cal {
            in_call: false,
            next_call: Some(NextEvent {
                start_time: now() + secs(90),
                ..event(0)
            }),
        };
        assert_eq!(step(&c, now(), now(), false).sleep, Duration::from_secs(60));
        // 30s to start: wake exactly at start
        let c = Cal {
            in_call: false,
            next_call: Some(NextEvent {
                start_time: now() + secs(30),
                ..event(0)
            }),
        };
        assert_eq!(step(&c, now(), now(), false).sleep, Duration::from_secs(30));
        // 65 min away: idle cap applies, but the hour-out boundary is never
        // overshot - a later tick at 62 min away sleeps exactly 2 min
        assert_eq!(step(&cal(65, false), now(), now(), false).sleep, DEFAULT_CHECK_INTERVAL);
        assert_eq!(
            step(&cal(62, false), now(), now(), false).sleep,
            Duration::from_secs(2 * 60)
        );
        // in a call: camera poll cadence
        assert_eq!(step(&cal(-2, true), now(), now(), false).sleep, CAMERA_POLL_INTERVAL);
        // waking exactly on a boundary: the 1s floor guards zero-length sleeps
        let c = Cal {
            in_call: false,
            next_call: Some(NextEvent {
                start_time: now(),
                ..event(0)
            }),
        };
        assert!(step(&c, now(), now() - secs(5), false).sleep >= Duration::from_secs(1));
    }

    #[test]
    fn status_lines() {
        assert_eq!(step(&Cal::default(), now(), now(), false).status, "No upcoming calls");
        assert!(
            step(&cal(30, false), now(), now(), false)
                .status
                .starts_with("Next: standup at ")
        );
        assert!(
            step(&cal(-2, true), now(), now(), false)
                .status
                .starts_with("In progress: standup (started ")
        );
    }
}

use crate::{
    camera,
    ical::{self, NextEvent},
    notifications, say, tray,
};
use anyhow::Result as AnyhowResult;
use chrono::{TimeDelta, Timelike, Utc};
use std::{
    borrow::Cow,
    thread::sleep,
    time::{Duration, Instant},
};
use tracing::{error, info, warn};

/// Default calendar re-check interval: 3 minutes.
pub const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_secs(180);

/// How often to re-check the camera while a call is in progress, so the tray
/// reflects joining/leaving the call within seconds.
const CAMERA_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Sleeps until `deadline`, polling the camera every few seconds and updating
/// the tray: person icon while the camera is active, `countdown` text while
/// not. Used instead of a plain sleep while a started event is in progress.
pub fn watch_camera_until(deadline: Instant, countdown: &str) {
    loop {
        if camera::camera_active() {
            tray::show_person();
        } else {
            tray::set_title(countdown);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        sleep(remaining.min(CAMERA_POLL_INTERVAL));
    }
}

/// What the background loop should do after updating the tray countdown.
#[derive(Debug)]
pub enum StepNext {
    Sleep(Duration),
    EventStarted(ical::NextEvent),
}

/// Outcome of [`calc_sleep`]: the tray text to show and the next action.
#[derive(Debug)]
pub struct StepResult {
    pub icon_text: Cow<'static, str>,
    pub next: StepNext,
}

/// Fetches the calendar and returns the relevant events (next + in-progress).
/// Fetch errors are surfaced as notifications and the previous events are
/// kept, so a transient network blip doesn't lose an imminent alert.
pub fn find_events(ics_url: &str, previous_events: ical::CalendarEvents) -> ical::CalendarEvents {
    let start = Instant::now();
    let request_result = ical::get_events(ics_url);
    let request_duration = start.elapsed();
    match request_result {
        Ok(events) => {
            if events.next.is_none() {
                info!("Got calendar in {request_duration:?}, No upcoming calls with video links");
            }
            events
        }
        Err(ical::CalendarError::HttpStatus(err)) => {
            error!("Got calendar in {request_duration:?}, HTTP error fetching calendar: {err}");
            notifications::send("Next Call", Some("HTTP error fetching calendar"), &err, None);
            previous_events
        }
        Err(ical::CalendarError::InvalidFormat(err)) => {
            error!("Got calendar in {request_duration:?}, Invalid iCal format: {err}");
            notifications::send("Next Call", Some("Invalid ical response"), &err, None);
            previous_events
        }
        Err(ical::CalendarError::NetworkError(err)) => {
            warn!("Got calendar in {request_duration:?}, Network error fetching calendar: {err}");
            notifications::send("Next Call", Some("Network error fetching calendar"), &err, None);
            previous_events
        }
    }
}

/// One-line summary of the calendar state, shown at the top of the tray menu.
/// Shows the in-progress call only during its alert window (the first
/// [`ical::NEXT_MAX_AGE_MINUTES`]); after that the next upcoming call is more
/// useful, even while still on the current one.
pub fn status_line(events: &ical::CalendarEvents) -> String {
    let local_start = |e: &NextEvent| e.start_time.with_timezone(&chrono::Local).format("%H:%M");
    if let Some(ref event) = events.in_progress
        && Utc::now().signed_duration_since(event.start_time).num_minutes() <= ical::NEXT_MAX_AGE_MINUTES
    {
        format!("In progress: {} (started {})", event.summary, local_start(event))
    } else if let Some(ref event) = events.next {
        format!("Next: {} at {}", event.summary, local_start(event))
    } else {
        "No upcoming calls".to_string()
    }
}

/// Decides the tray countdown text and how long to sleep before re-checking:
/// "..." when >60 min away, minutes remaining when closer, and
/// `EventStarted` once the start time has passed.
pub fn calc_sleep(next_event: &ical::NextEvent) -> AnyhowResult<StepResult> {
    let now = Utc::now();
    let until_start = next_event.start_time.signed_duration_since(now);
    if until_start < TimeDelta::zero() {
        info!("next call {:?} started", next_event.summary);
        return Ok(StepResult {
            icon_text: format!("{:.0}", until_start.as_seconds_f32() / 60.0).into(),
            next: StepNext::EventStarted(next_event.clone()),
        });
    }

    // minutes rounded down
    let minutes_until = (until_start.as_seconds_f32() / 60.0).floor() as i32;

    let (icon_text, sleep_duration) = if minutes_until <= 60 {
        (
            minutes_until.to_string().into(),
            // duration until the top of the minute
            until_start.to_std()?.min(Duration::from_secs(60 - now.second() as u64)),
        )
    } else {
        // More than 60 minutes away
        let hour_before = next_event.start_time - TimeDelta::hours(1);
        let until_hour_before = hour_before.signed_duration_since(now).to_std()?;
        ("...".into(), DEFAULT_CHECK_INTERVAL.min(until_hour_before))
    };

    info!(
        "next call {:?} starts at {:?} in {:.2}s, waiting for {:?}",
        next_event.summary,
        next_event.start_time,
        until_start.as_seconds_f32(),
        sleep_duration
    );
    Ok(StepResult {
        icon_text,
        next: StepNext::Sleep(sleep_duration),
    })
}

/// Runs the alert sequence for a started event: notify immediately, then show
/// a negative minutes countdown in the tray, re-notifying at +2 and +5 min.
/// Stops early once the camera is active (the user has joined the call).
pub fn event_started(event: NextEvent, eleven_labs_key: Option<&str>) -> AnyhowResult<()> {
    info!("Event {:?} has started", event.summary);

    if maybe_notify(&event, eleven_labs_key, true)? {
        // camera active, stop
        return Ok(());
    }

    for i in 0..5 {
        let minutes = Utc::now().signed_duration_since(event.start_time).to_std()?.as_secs() as f32 / 60.0;
        tray::set_title(&format!("-{minutes:.0}"));
        if i == 2 && maybe_notify(&event, eleven_labs_key, false)? {
            // camera active, stop
            return Ok(());
        }
        // sleep until the start of the next minute, polling the camera so
        // that joining the call stops the alerts within seconds
        let until_min_end = Duration::from_secs(60 - Utc::now().second() as u64);
        let deadline = Instant::now() + until_min_end;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            sleep(remaining.min(CAMERA_POLL_INTERVAL));
            if camera::camera_active() {
                // user joined the call: show the person icon, stop alerting
                tray::show_person();
                return Ok(());
            }
        }
    }

    maybe_notify(&event, eleven_labs_key, false)?;
    Ok(())
}

/// Sends a notification and speaks an announcement unless the camera is
/// already active (i.e. the user is on the call). Returns whether it was.
fn maybe_notify(event: &NextEvent, eleven_labs_key: Option<&str>, always_notify: bool) -> AnyhowResult<bool> {
    let camera_active = camera::camera_active();
    let since_start = Utc::now().signed_duration_since(event.start_time).to_std()?;
    info!(
        "Event {:?} {:?} notification, camera active: {:?}",
        event.summary, since_start, camera_active
    );
    let minutes = since_start.as_secs() as f32 / 60.0;

    // Every reminder after the start alert ends with a call to action.
    let started_description: Cow<'static, str> = if minutes < 1.0 {
        "has started".into()
    } else {
        format!("started {minutes:.0} minutes ago, join it now!").into()
    };

    if !camera_active || always_notify {
        notifications::send(
            "Nextcall",
            Some(&format!("Call {started_description}")),
            &event.summary,
            Some(&event.video_link),
        );
    }
    if !camera_active {
        let message = format!("Your call {:?} {}", sayevent_summary(event), started_description);
        let _ = say::say(&message, eleven_labs_key);
    }
    Ok(camera_active)
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

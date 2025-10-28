use crate::{
    camera,
    ical::{self, NextEvent},
    notifications, say,
};
use anyhow::Result as AnyhowResult;
use chrono::{TimeDelta, Timelike, Utc};
use std::{
    borrow::Cow,
    sync::mpsc::Sender,
    thread::sleep,
    time::{Duration, Instant},
};
use tracing::{error, info, warn};

// Default check interval: 3 minutes
const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_secs(180);

#[derive(Debug)]
pub enum StepNext {
    Sleep(Duration),
    EventStarted(ical::NextEvent),
}

#[derive(Debug)]
pub struct StepResult {
    pub icon_text: Cow<'static, str>,
    pub next: StepNext,
}

pub fn find_next_event(
    ics_url: &str,
    first_run: bool,
    previous_next_event: Option<ical::NextEvent>,
) -> Option<ical::NextEvent> {
    info!("Checking calendar for upcoming events");
    let start = Instant::now();
    let request_result = ical::get_next_event(ics_url, first_run);
    let request_duration = start.elapsed();
    match request_result {
        Ok(event) => {
            info!("Got calendar in {request_duration:?}, next call {:?}", event.summary);
            Some(event)
        }
        Err(ical::CalendarError::HttpStatus(err)) => {
            error!("Got calendar in {request_duration:?}, HTTP error fetching calendar: {err}");
            notifications::send("Next Call", Some("HTTP error fetching calendar"), &err, None);
            previous_next_event
        }
        Err(ical::CalendarError::InvalidFormat(err)) => {
            error!("Got calendar in {request_duration:?}, Invalid iCal format: {err}");
            notifications::send("Next Call", Some("Invalid ical response"), &err, None);
            previous_next_event
        }
        Err(ical::CalendarError::NetworkError(err)) => {
            warn!("Got calendar in {request_duration:?}, Network error fetching calendar: {err}");
            notifications::send("Next Call", Some("Network error fetching calendar"), &err, None);
            previous_next_event
        }
        Err(ical::CalendarError::NoUpcomingEvents) => {
            info!("Got calendar in {request_duration:?}, No upcoming calls with video links");
            None
        }
    }
}

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

pub fn event_started(
    event: NextEvent,
    eleven_labs_key: Option<&str>,
    icon_tx: &Sender<Cow<'static, str>>,
) -> AnyhowResult<()> {
    info!("Event {:?} has started", event.summary);

    maybe_notify(&event, eleven_labs_key, true)?;

    for i in 0..5 {
        let minutes = Utc::now().signed_duration_since(event.start_time).to_std()?.as_secs() as f32 / 60.0;
        icon_tx.send(format!("-{minutes:.0}").into())?;
        if i == 2 {
            maybe_notify(&event, eleven_labs_key, false)?
        }
        // sleep until the top of the next minute
        let until_min_end = Duration::from_secs(60 - Utc::now().second() as u64);
        sleep(until_min_end);
    }

    maybe_notify(&event, eleven_labs_key, false)
}

fn maybe_notify(event: &NextEvent, eleven_labs_key: Option<&str>, always_notify: bool) -> AnyhowResult<()> {
    let camera_active = camera::camera_active();
    let since_start = Utc::now().signed_duration_since(event.start_time).to_std()?;
    info!(
        "Event {:?} {:?} notification, camera active: {:?}",
        event.summary, since_start, camera_active
    );
    let minutes = since_start.as_secs() as f32 / 60.0;
    if !camera_active || always_notify {
        notifications::send(
            "Nextcall",
            Some(&format!("Call Started {}", time_since_description(minutes))),
            &event.summary,
            Some(&event.video_link),
        );
    }
    if !camera_active {
        let message = format!(
            "Your call {:?} started {}{}",
            sayevent_summary(event),
            time_since_description(minutes),
            if minutes > 1.0 { ", join it now!" } else { "" }
        );
        let _ = say::say(&message, eleven_labs_key);
    }
    Ok(())
}

fn time_since_description(minutes: f32) -> Cow<'static, str> {
    if minutes < 1.0 {
        "just now".into()
    } else {
        format!("{minutes:.0} minutes ago").into()
    }
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

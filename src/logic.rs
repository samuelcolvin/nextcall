use crate::{
    camera,
    ical::{self, NextEvent},
    notifications, say,
};
use anyhow::Result as AnyhowResult;
use chrono::{TimeDelta, Timelike, Utc};
use std::{borrow::Cow, thread::sleep, time::Duration};
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

impl StepResult {
    fn network_error() -> Self {
        Self {
            icon_text: "E".into(),
            next: StepNext::Sleep(DEFAULT_CHECK_INTERVAL),
        }
    }
}

pub fn find_next_event(ics_url: &str) -> AnyhowResult<StepResult> {
    info!("Checking calendar for upcoming events");
    let next_event = match ical::get_next_event(ics_url) {
        Ok(event) => event,
        Err(ical::CalendarError::HttpStatus(err)) => {
            error!("HTTP error fetching calendar: {}", err);
            notifications::send("Next Call", Some("Fetch failed"), &err, None);
            return Ok(StepResult::network_error());
        }
        Err(ical::CalendarError::InvalidFormat(err)) => {
            error!("Invalid iCal format: {}", err);
            notifications::send("Next Call", Some("Invalid ical response"), &err, None);
            return Ok(StepResult::network_error());
        }
        Err(ical::CalendarError::NetworkError(err)) => {
            warn!("Network error fetching calendar: {}", err);
            return Ok(StepResult::network_error());
        }
        Err(ical::CalendarError::NoUpcomingEvents) => {
            info!("No upcoming calls with video links");
            return Ok(StepResult {
                icon_text: "...".into(),
                next: StepNext::Sleep(DEFAULT_CHECK_INTERVAL),
            });
        }
    };

    let now = Utc::now();
    let duration_until_start = next_event.start_time.signed_duration_since(now).to_std()?;
    // minutes rounded down
    let minutes_until = (duration_until_start.as_secs() as f32 / 60.0).floor() as i32;

    let (icon_text, sleep_duration) = if minutes_until <= 60 {
        (
            minutes_until.to_string().into(),
            // duration until the top of the minute
            duration_until_start.min(Duration::from_secs(60 - now.second() as u64)),
        )
    } else {
        // More than 60 minutes away
        let hour_before = next_event.start_time - TimeDelta::hours(1);
        let until_hour_before = hour_before.signed_duration_since(now).to_std()?;
        ("...".into(), DEFAULT_CHECK_INTERVAL.min(until_hour_before))
    };

    info!(
        "Next call \"{}\" starts at {:?} in {:?}, waiting for {:?}",
        next_event.summary, next_event.start_time, duration_until_start, sleep_duration
    );
    Ok(StepResult {
        icon_text,
        next: if duration_until_start > Duration::ZERO {
            StepNext::Sleep(sleep_duration)
        } else {
            StepNext::EventStarted(next_event)
        },
    })
}

pub fn event_started(event: NextEvent, eleven_labs_key: Option<&str>) -> AnyhowResult<()> {
    info!("Event \"{}\" has just started", event.summary);

    notifications::send(
        "Nextcall",
        Some("Call just started"),
        &event.summary,
        Some(&event.video_link),
    );
    if !camera::camera_active() {
        let message = format!("Your call \"{}\" has just started", event.summary);
        let _ = say::say(&message, eleven_labs_key);
    }

    sleep(Duration::from_secs(120));

    maybe_notify(&event, eleven_labs_key)?;

    sleep(Duration::from_secs(180));

    maybe_notify(&event, eleven_labs_key)
}

fn maybe_notify(event: &NextEvent, eleven_labs_key: Option<&str>) -> AnyhowResult<()> {
    let camera_active = camera::camera_active();
    let since_start = event.start_time.signed_duration_since(Utc::now()).to_std()?;
    info!(
        "Event \"{}\" {:?} notification, camera active: {:?}",
        event.summary, since_start, camera_active
    );
    if !camera_active {
        // minutes rounded to nearest
        let minutes = (since_start.as_secs() as f32 / 60.0).round() as i32;

        notifications::send(
            "Nextcall",
            Some(&format!("Call Started {minutes} minutes ago")),
            &event.summary,
            Some(&event.video_link),
        );
        let message = format!(
            "Your call \"{}\" started {} minutes ago, join it now!",
            event.summary, minutes
        );
        let _ = say::say(&message, eleven_labs_key);
    }
    Ok(())
}

use crate::{camera, ical, icon, notifications, say};
use anyhow::Result as AnyhowResult;
use chrono::{DateTime, Utc};
use std::time::Duration;

// Default check interval: 3 minutes
const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_secs(180);

#[derive(Debug, Clone)]
pub struct ActiveEvent {
    pub summary: String,
    pub start_time: DateTime<Utc>,
    pub video_link: Option<String>,
    pub notified_at_start: bool,
    pub notified_at_2min: bool,
    pub notified_at_5min: bool,
}

impl ActiveEvent {
    pub fn new(summary: String, start_time: DateTime<Utc>, video_link: Option<String>) -> Self {
        Self {
            summary,
            start_time,
            video_link,
            notified_at_start: false,
            notified_at_2min: false,
            notified_at_5min: false,
        }
    }

    pub fn minutes_since_start(&self) -> i64 {
        let now = Utc::now();
        let duration = now.signed_duration_since(self.start_time);
        duration.num_minutes()
    }
}

pub struct StepResult {
    pub new_icon: Option<tray_icon::Icon>,
    pub new_active_event: Option<ActiveEvent>,
    pub next_check_duration: Duration,
}

pub fn step(
    ics_url: &str,
    eleven_labs_key: Option<&str>,
    mut current_active_event: Option<&mut ActiveEvent>,
) -> AnyhowResult<StepResult> {
    let next_event = match ical::get_next_event(ics_url) {
        Ok(event) => event,
        Err(ical::CalendarError::HttpStatus(err)) => {
            notifications::send("Next Call", Some("Invalid URL"), &err, None);
            return Ok(StepResult {
                new_icon: None,
                new_active_event: None,
                next_check_duration: DEFAULT_CHECK_INTERVAL,
            });
        }
        Err(ical::CalendarError::InvalidFormat(err)) => {
            notifications::send("Next Call", Some("Invalid ical response"), &err, None);
            return Ok(StepResult {
                new_icon: None,
                new_active_event: None,
                next_check_duration: DEFAULT_CHECK_INTERVAL,
            });
        }
        Err(ical::CalendarError::NetworkError(err)) => {
            eprintln!("Network error: {}", err);
            return Ok(StepResult {
                new_icon: None,
                new_active_event: None,
                next_check_duration: DEFAULT_CHECK_INTERVAL,
            });
        }
        Err(ical::CalendarError::NoUpcomingEvents) => {
            println!("No upcoming calls with video links");
            return Ok(StepResult {
                new_icon: Some(icon::create_icon_with_text("...", false)),
                new_active_event: None,
                next_check_duration: DEFAULT_CHECK_INTERVAL,
            });
        }
    };

    let now = Utc::now();
    let time_until_start = next_event.start_time.signed_duration_since(now);
    let minutes_until = time_until_start.num_minutes();

    // Print status for events in the future
    if minutes_until > 0 {
        println!(
            "Next call \"{}\" starts at {} in {}",
            next_event.summary,
            next_event.start_time.format("%Y-%m-%d %H:%M:%S %Z"),
            display_interval(time_until_start.num_seconds())
        );
    }

    let new_icon = if minutes_until <= 0 {
        // Event has started
        icon::create_icon_with_text("0", true)
    } else if minutes_until <= 60 {
        // Show countdown
        icon::create_icon_with_text(&minutes_until.to_string(), false)
    } else {
        // More than 60 minutes away
        icon::create_icon_with_text("...", false)
    };

    // Check if this is a new event that just started
    if (-10..=0).contains(&minutes_until) {
        // Check if we need to create a new active event or if it's the same one
        let is_new_event = match current_active_event.as_ref() {
            Some(active) => active.summary != next_event.summary || active.start_time != next_event.start_time,
            None => true,
        };

        if is_new_event {
            // New event started
            println!(
                "Starting event sequence for event \"{}\" starting at {}...",
                next_event.summary,
                next_event.start_time.format("%Y-%m-%d %H:%M:%S %Z")
            );
            let mut new_active =
                ActiveEvent::new(next_event.summary, next_event.start_time, Some(next_event.video_link));
            send_event_alert(&new_active, eleven_labs_key);
            new_active.notified_at_start = true;

            let next_check =
                calculate_next_check_duration(minutes_until, time_until_start.num_seconds(), Some(&new_active));

            return Ok(StepResult {
                new_icon: Some(new_icon),
                new_active_event: Some(new_active),
                next_check_duration: next_check,
            });
        } else if let Some(ref mut active) = current_active_event {
            // Same event, check if we need to send reminders
            check_and_send_reminders(active, eleven_labs_key);
        }
    }

    // Calculate next check duration based on current state
    let next_check = calculate_next_check_duration(
        minutes_until,
        time_until_start.num_seconds(),
        current_active_event.as_deref(),
    );

    Ok(StepResult {
        new_icon: Some(new_icon),
        new_active_event: None,
        next_check_duration: next_check,
    })
}

fn send_event_alert(event: &ActiveEvent, eleven_labs_key: Option<&str>) {
    let minutes = event.minutes_since_start();

    if camera::camera_active() {
        println!(
            "Skipping {} minute notification for \"{}\", camera active",
            minutes, event.summary
        );
        return;
    }

    if minutes <= 0 {
        // Call just started
        notifications::send(
            "Call has just started",
            Some(&event.summary),
            "",
            event.video_link.as_deref(),
        );
        let message = format!("Your call \"{}\" has just started", event.summary);
        let _ = say::say(&message, eleven_labs_key);
    } else {
        // Call started X minutes ago
        let minute_word = if minutes == 1 { "minute" } else { "minutes" };
        notifications::send(
            &format!("Call started {} {} ago", minutes, minute_word),
            Some(&event.summary),
            "",
            event.video_link.as_deref(),
        );
        let minutes_word = int_as_word(minutes as usize);
        let message = format!(
            "Your call \"{}\" started {} {} ago, JOIN IT NOW!",
            event.summary, minutes_word, minute_word
        );
        let _ = say::say(&message, eleven_labs_key);
    }
}

fn check_and_send_reminders(active_event: &mut ActiveEvent, eleven_labs_key: Option<&str>) {
    let minutes = active_event.minutes_since_start();

    if minutes >= 2 && !active_event.notified_at_2min {
        send_event_alert(active_event, eleven_labs_key);
        active_event.notified_at_2min = true;
    } else if minutes >= 5 && !active_event.notified_at_5min {
        send_event_alert(active_event, eleven_labs_key);
        active_event.notified_at_5min = true;
    }
}

fn int_as_word(n: usize) -> String {
    let words = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    ];
    if n < words.len() {
        words[n].to_string()
    } else {
        n.to_string()
    }
}

fn calculate_next_check_duration(
    minutes_until_event: i64,
    seconds_until_event: i64,
    active_event: Option<&ActiveEvent>,
) -> Duration {
    // If event is in the future
    if minutes_until_event > 0 {
        if minutes_until_event > 60 {
            // Event is more than 60 minutes away, use default interval (3 minutes)
            DEFAULT_CHECK_INTERVAL
        } else if minutes_until_event < 3 {
            // Event starts within 3 minutes, schedule check exactly at event start
            // Use exact seconds, but ensure at least 1 second
            Duration::from_secs(seconds_until_event.max(1) as u64)
        } else {
            // Event is between 3 and 60 minutes away, check every minute to update countdown
            // Calculate seconds until the next minute boundary
            let seconds_in_current_minute = seconds_until_event % 60;
            let seconds_until_next_minute = if seconds_in_current_minute == 0 {
                60
            } else {
                seconds_in_current_minute
            };
            Duration::from_secs(seconds_until_next_minute as u64)
        }
    } else if let Some(active) = active_event {
        // Event has started, calculate when next reminder is due
        let minutes_since = active.minutes_since_start();

        if !active.notified_at_2min && minutes_since < 2 {
            // Next reminder at 2 minutes
            Duration::from_secs((2 - minutes_since) as u64 * 60)
        } else if !active.notified_at_5min && minutes_since < 5 {
            // Next reminder at 5 minutes
            Duration::from_secs((5 - minutes_since) as u64 * 60)
        } else {
            // All reminders sent, use default interval
            DEFAULT_CHECK_INTERVAL
        }
    } else {
        // Event has started but no active event tracked, check soon
        Duration::from_secs(10)
    }
}

fn display_interval(seconds: i64) -> String {
    let minutes = seconds / 60;
    let hours = seconds / 3600;
    let days = seconds / 86400;

    if seconds < 60 {
        "less than a minute".to_string()
    } else if seconds < 3600 {
        let plural = if minutes != 1 { "s" } else { "" };
        format!("{} minute{}", minutes, plural)
    } else if seconds < 86400 {
        let plural = if hours != 1 { "s" } else { "" };
        format!("{} hour{}", int_as_word(hours as usize), plural)
    } else if seconds < 172800 {
        // Less than 2 days
        let remaining_hours = (seconds - 86400) / 3600;
        let plural = if remaining_hours != 1 { "s" } else { "" };
        format!("1 day, {} hour{}", int_as_word(remaining_hours as usize), plural)
    } else {
        let plural = if days != 1 { "s" } else { "" };
        format!("{} day{}", int_as_word(days as usize), plural)
    }
}

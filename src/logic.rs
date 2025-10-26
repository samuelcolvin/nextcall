use crate::ical::IcalEvent;
use crate::{camera, ical, icon, notifications, say};
use anyhow::Result as AnyhowResult;
use chrono::{DateTime, Utc};

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
}

pub fn step(
    ics_url: &str,
    eleven_labs_key: Option<&str>,
    current_active_event: Option<&mut ActiveEvent>,
) -> AnyhowResult<StepResult> {
    let calendar = match ical::get_ics(ics_url) {
        Ok(calendar) => calendar,
        Err(ical::CalendarError::HttpStatus(err)) => {
            notifications::send("Next Call", Some("Invalid URL"), &err, None);
            return Ok(StepResult {
                new_icon: None,
                new_active_event: None,
            });
        }
        Err(ical::CalendarError::InvalidFormat(err)) => {
            notifications::send("Next Call", Some("Invalid ical response"), &err, None);
            return Ok(StepResult {
                new_icon: None,
                new_active_event: None,
            });
        }
        Err(ical::CalendarError::NetworkError(err)) => {
            eprintln!("Network error: {}", err);
            return Ok(StepResult {
                new_icon: None,
                new_active_event: None,
            });
        }
    };

    let now = Utc::now();

    // Filter events that have video links and are in the future or recently started
    let mut relevant_events: Vec<&(DateTime<Utc>, IcalEvent)> = calendar
        .events
        .iter()
        .filter(|(start_time, event)| {
            let has_video = ical::get_video_link(event).is_some();
            let minutes_diff = start_time.signed_duration_since(now).num_minutes();
            has_video && minutes_diff >= -10 // Include events that started up to 10 minutes ago
        })
        .collect();

    if relevant_events.is_empty() {
        println!("No upcoming calls with video links");
        return Ok(StepResult {
            new_icon: Some(icon::create_icon_with_text("...", false)),
            new_active_event: None,
        });
    }

    // Sort by start time to ensure we get the chronologically next event
    relevant_events.sort_by_key(|(start_time, _)| *start_time);

    // Get the next event
    let (next_start, next_event) = relevant_events[0];
    let time_until_start = next_start.signed_duration_since(now);
    let minutes_until = time_until_start.num_minutes();

    let event_summary = ical::get_event_summary(next_event).unwrap_or_else(|| "Unknown".to_string());

    // Print status for events in the future
    if minutes_until > 0 {
        println!(
            "Next call \"{}\" starts at {} in {}, waiting 10 seconds",
            event_summary,
            next_start.format("%Y-%m-%d %H:%M:%S %Z"),
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
        let video_link = ical::get_video_link(next_event);

        // Check if we need to create a new active event or if it's the same one
        let is_new_event = match current_active_event.as_ref() {
            Some(active) => active.summary != event_summary || active.start_time != *next_start,
            None => true,
        };

        if is_new_event {
            // New event started
            println!(
                "Starting event sequence for event \"{}\" starting at {}...",
                event_summary,
                next_start.format("%Y-%m-%d %H:%M:%S %Z")
            );
            let mut new_active = ActiveEvent::new(event_summary, *next_start, video_link);
            send_event_alert(&new_active, eleven_labs_key);
            new_active.notified_at_start = true;

            return Ok(StepResult {
                new_icon: Some(new_icon),
                new_active_event: Some(new_active),
            });
        } else if let Some(active) = current_active_event {
            // Same event, check if we need to send reminders
            check_and_send_reminders(active, eleven_labs_key);
        }
    }

    Ok(StepResult {
        new_icon: Some(new_icon),
        new_active_event: None,
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

use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use ical::IcalParser;
pub use ical::parser::ical::component::IcalEvent;
use std::io::BufReader;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub struct NextEvent {
    pub start_time: DateTime<Utc>,
    pub summary: String,
    pub video_link: String,
}

/// The calendar events the app cares about right now. Both may point at the
/// same event (one that started a few minutes ago).
#[derive(Debug, Clone, Default)]
pub struct CalendarEvents {
    /// Earliest event that is upcoming or started within the alert window;
    /// drives the countdown and the alert sequence.
    pub next: Option<NextEvent>,
    /// Most recently started event within the last hour; drives the person
    /// icon while the camera is active.
    pub in_progress: Option<NextEvent>,
}

#[derive(Debug)]
pub enum CalendarError {
    // for any status code > 400, should be `{status}: {text}`
    HttpStatus(String),
    // invalid ics file,
    InvalidFormat(String),
    // Other network errors
    NetworkError(String),
}

/// Alert window: events that started less than this many minutes ago still
/// count as `next`, so their alert sequence can run (it ends at +5 minutes).
pub const NEXT_MAX_AGE_MINUTES: i64 = 10;

/// In-progress window: events that started within the last hour count as
/// `in_progress`, so being on the call shows the person icon for its
/// (assumed ~1h) duration.
const IN_PROGRESS_MAX_AGE_MINUTES: i64 = 60;

/// Fetches and parses the iCal feed, returning the relevant events (see
/// [`CalendarEvents`]). Only events with video links are considered.
pub fn get_events(url: &str) -> Result<CalendarEvents, CalendarError> {
    // Download the iCal file
    let response = reqwest::blocking::get(url).map_err(|e| CalendarError::NetworkError(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let status_text = response.text().unwrap_or_default();
        return Err(CalendarError::HttpStatus(format!("{status}: {status_text}",)));
    }

    let content = response
        .bytes()
        .map_err(|e| CalendarError::NetworkError(e.to_string()))?;

    let reader = BufReader::new(content.as_ref());

    // Parse the iCal file
    let parser = IcalParser::new(reader);
    let mut events = CalendarEvents::default();
    let now = Utc::now();

    for calendar in parser {
        match calendar {
            Ok(cal) => {
                for event in cal.events {
                    let Some(start_time) = extract_datetime(&event) else {
                        continue;
                    };
                    // positive = the event started that long ago
                    let age = now.signed_duration_since(start_time);
                    if age.num_minutes() > IN_PROGRESS_MAX_AGE_MINUTES {
                        continue;
                    }
                    let Some(video_link) = get_video_link(&event) else {
                        continue;
                    };
                    let candidate = NextEvent {
                        start_time,
                        summary: get_event_summary(&event).unwrap_or_else(|| "Unknown".to_string()),
                        video_link,
                    };
                    // next: the earliest upcoming or recently started event
                    if age.num_minutes() <= NEXT_MAX_AGE_MINUTES
                        && events.next.as_ref().is_none_or(|n| candidate.start_time < n.start_time)
                    {
                        events.next = Some(candidate.clone());
                    }
                    // in_progress: the most recently *started* event
                    if age.num_seconds() >= 0
                        && events
                            .in_progress
                            .as_ref()
                            .is_none_or(|p| candidate.start_time > p.start_time)
                    {
                        events.in_progress = Some(candidate);
                    }
                }
            }
            Err(e) => {
                return Err(CalendarError::InvalidFormat(e.to_string()));
            }
        }
    }
    Ok(events)
}

fn extract_datetime(event: &IcalEvent) -> Option<DateTime<Utc>> {
    // First, find the DTSTART property
    let dtstart_property = event.properties.iter().find(|p| p.name == "DTSTART")?;
    let value = dtstart_property.value.as_ref()?;

    // Check if there's a TZID parameter
    let tzid = dtstart_property.params.as_ref().and_then(|params| {
        params.iter().find_map(|(key, values)| {
            if key == "TZID" && !values.is_empty() {
                Some(values[0].as_str())
            } else {
                None
            }
        })
    });

    // Clean the datetime string
    let cleaned = value.replace("-", "").replace(":", "");

    // Handle timezone-aware datetime
    if let Some(tz_name) = tzid {
        // Parse timezone: YYYYMMDDTHHMMSS
        if cleaned.contains('T') && cleaned.len() >= 15 {
            let dt_str = &cleaned[..15]; // Take YYYYMMDDTHHMMSS
            if let Ok(naive_dt) = chrono::NaiveDateTime::parse_from_str(dt_str, "%Y%m%dT%H%M%S") {
                // Parse the timezone
                if let Ok(tz) = Tz::from_str(tz_name) {
                    // Convert to UTC
                    if let Some(local_dt) = tz.from_local_datetime(&naive_dt).earliest() {
                        return Some(local_dt.with_timezone(&Utc));
                    }
                }
            }
        }
        return None;
    }

    // Handle UTC datetime (ends with Z)
    if cleaned.contains('T') && cleaned.ends_with('Z') {
        // Format: 20231225T120000Z
        let dt_str = cleaned.trim_end_matches('Z');
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(dt_str, "%Y%m%dT%H%M%S") {
            return Some(DateTime::from_naive_utc_and_offset(dt, Utc));
        }
    }

    // Handle date only (no time)
    if cleaned.len() == 8 {
        // Format: 20231225 (date only)
        if let Ok(date) = chrono::NaiveDate::parse_from_str(&cleaned, "%Y%m%d") {
            let dt = date.and_hms_opt(0, 0, 0)?;
            return Some(DateTime::from_naive_utc_and_offset(dt, Utc));
        }
    }

    None
}

pub fn get_property(event: &IcalEvent, name: &str) -> Option<String> {
    event
        .properties
        .iter()
        .find(|p| p.name == name)
        .and_then(|p| p.value.clone())
}

pub fn get_event_summary(event: &IcalEvent) -> Option<String> {
    get_property(event, "SUMMARY")
}

pub fn get_video_link(event: &IcalEvent) -> Option<String> {
    // Check for X-GOOGLE-CONFERENCE property (Google Calendar)
    if let Some(url) = get_property(event, "X-GOOGLE-CONFERENCE")
        && url.starts_with("http")
    {
        return Some(url);
    }

    // Check for URL property (Zoom, Teams, etc.)
    if let Some(url) = get_property(event, "URL")
        && url.starts_with("http")
    {
        return Some(url);
    }

    // Check location field
    if let Some(location) = get_property(event, "LOCATION")
        && location.starts_with("http")
    {
        return Some(location);
    }

    // Check description for meeting links
    if let Some(description) = get_property(event, "DESCRIPTION") {
        // Look for common video conferencing URLs
        for line in description.lines() {
            // Check if line contains a video conferencing URL
            if line.contains("zoom.us") || line.contains("meet.google.com") || line.contains("teams.microsoft.com") {
                // Extract the URL from the line
                if let Some(start) = line.find("http") {
                    let url_part = &line[start..];
                    // Find the end of the URL (space, newline, or end of string)
                    let end = url_part.find(|c: char| c.is_whitespace()).unwrap_or(url_part.len());
                    return Some(url_part[..end].to_string());
                }
            }
        }
    }

    None
}

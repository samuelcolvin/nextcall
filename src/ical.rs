use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use ical::IcalParser;
pub use ical::parser::ical::component::IcalEvent;
use std::io::BufReader;
use std::str::FromStr;

#[derive(Debug)]
pub struct NextEvent {
    pub start_time: DateTime<Utc>,
    pub summary: String,
    pub video_link: String,
}

#[derive(Debug)]
pub enum CalendarError {
    // for any status code > 400, should be `{status}: {text}`
    HttpStatus(String),
    // invalid ics file,
    InvalidFormat(String),
    // Other network errors
    NetworkError(String),
    // No upcoming events with video links
    NoUpcomingEvents,
}

pub fn get_next_event(url: &str) -> Result<NextEvent, CalendarError> {
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
    let mut events = Vec::new();

    for calendar in parser {
        match calendar {
            Ok(cal) => {
                for event in cal.events {
                    if let Some(start_time) = extract_datetime(&event) {
                        events.push((start_time, event));
                    }
                }
            }
            Err(e) => {
                return Err(CalendarError::InvalidFormat(e.to_string()));
            }
        }
    }

    // Sort events by start time
    events.sort_by(|a, b| a.0.cmp(&b.0));

    // Get current time
    let now = Utc::now();

    // Filter events that have video links and are in the future or recently started (within 10 minutes)
    let next_event = events.into_iter().find(|(start_time, event)| {
        let has_video = get_video_link(event).is_some();
        let minutes_diff = start_time.signed_duration_since(now).num_minutes();
        has_video && minutes_diff >= -10 // Include events that started up to 10 minutes ago
    });

    match next_event {
        Some((start_time, event)) => {
            let summary = get_event_summary(&event).unwrap_or_else(|| "Unknown".to_string());
            let video_link = get_video_link(&event).expect("Event should have video link");

            Ok(NextEvent {
                start_time,
                summary,
                video_link,
            })
        }
        None => Err(CalendarError::NoUpcomingEvents),
    }
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
    if let Some(url) = get_property(event, "X-GOOGLE-CONFERENCE") {
        if url.starts_with("http") {
            return Some(url);
        }
    }

    // Check for URL property (Zoom, Teams, etc.)
    if let Some(url) = get_property(event, "URL") {
        if url.starts_with("http") {
            return Some(url);
        }
    }

    // Check location field
    if let Some(location) = get_property(event, "LOCATION") {
        if location.starts_with("http") {
            return Some(location);
        }
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

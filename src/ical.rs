use chrono::{DateTime, Utc};
use ical::IcalParser;
use ical::parser::ical::component::IcalEvent;
use std::io::BufReader;

#[derive(Debug)]
pub struct Calendar {
    pub events: Vec<(DateTime<Utc>, IcalEvent)>,
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

pub fn get_ics(url: &str) -> Result<Calendar, CalendarError> {
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

    Ok(Calendar { events })
}

fn extract_datetime(event: &IcalEvent) -> Option<DateTime<Utc>> {
    for property in &event.properties {
        if property.name == "DTSTART" {
            if let Some(value) = &property.value {
                // Try to parse the datetime
                // iCal format: YYYYMMDDTHHMMSSZ or YYYYMMDD
                let cleaned = value.replace("-", "").replace(":", "");

                // Handle different formats
                if cleaned.contains('T') && cleaned.ends_with('Z') {
                    // Format: 20231225T120000Z
                    let dt_str = cleaned.trim_end_matches('Z');
                    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(dt_str, "%Y%m%dT%H%M%S") {
                        return Some(DateTime::from_naive_utc_and_offset(dt, Utc));
                    }
                } else if cleaned.len() == 8 {
                    // Format: 20231225 (date only)
                    if let Ok(date) = chrono::NaiveDate::parse_from_str(&cleaned, "%Y%m%d") {
                        let dt = date.and_hms_opt(0, 0, 0)?;
                        return Some(DateTime::from_naive_utc_and_offset(dt, Utc));
                    }
                }
            }
        }
    }
    None
}

#[allow(dead_code)]
fn get_property(event: &IcalEvent, name: &str) -> Option<String> {
    event
        .properties
        .iter()
        .find(|p| p.name == name)
        .and_then(|p| p.value.clone())
}

use chrono::{DateTime, Utc};
use ical::parser::ical::component::IcalEvent;
use ical::IcalParser;
use std::io::BufReader;
use std::env;

fn main() {
    // Get URL from command line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <ical-url>", args[0]);
        std::process::exit(1);
    }

    let url = &args[1];

    println!("Downloading iCal file from: {}", url);

    // Download the iCal file
    let response = reqwest::blocking::get(url)
        .expect("Failed to download iCal file");

    if !response.status().is_success() {
        eprintln!("Failed to download: HTTP {}", response.status());
        return;
    }

    let content = response.bytes().expect("Failed to read response");
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
                eprintln!("Error parsing calendar: {:?}", e);
            }
        }
    }

    // Sort events by start time
    events.sort_by(|a, b| a.0.cmp(&b.0));

    // Filter for upcoming events and take the next 5
    let now = Utc::now();
    let upcoming: Vec<_> = events
        .into_iter()
        .filter(|(dt, _)| dt > &now)
        .take(5)
        .collect();

    println!("\n=== Next 5 Upcoming Appointments ===\n");

    if upcoming.is_empty() {
        println!("No upcoming appointments found.");
    } else {
        for (i, (start_time, event)) in upcoming.iter().enumerate() {
            println!("--- Appointment {} ---", i + 1);
            println!("Start: {}", start_time);

            if let Some(summary) = get_property(&event, "SUMMARY") {
                println!("Summary: {}", summary);
            }

            if let Some(description) = get_property(&event, "DESCRIPTION") {
                println!("Description: {}", description);
            }

            if let Some(location) = get_property(&event, "LOCATION") {
                println!("Location: {}", location);
            }

            println!("\nFull event details: {:#?}\n", event);
        }
    }
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

fn get_property(event: &IcalEvent, name: &str) -> Option<String> {
    event.properties
        .iter()
        .find(|p| p.name == name)
        .and_then(|p| p.value.clone())
}

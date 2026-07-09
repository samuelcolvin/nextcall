use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use ical::IcalParser;
pub use ical::parser::ical::component::IcalEvent;
use ical::property::Property;
use std::collections::HashMap;
use std::io::BufReader;
use std::str::FromStr;
use tracing::warn;

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

/// How many upcoming occurrences of a recurring event to consider. 2 covers
/// the "one in progress + the next one upcoming" case (e.g. a daily standup).
const RECURRING_OCCURRENCE_LIMIT: u16 = 2;

/// Fetches and parses the iCal feed, returning the relevant events (see
/// [`CalendarEvents`]). Only events with video links are considered;
/// recurring events are expanded to their concrete occurrences.
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

    parse_events(content.as_ref(), Utc::now())
}

/// Parses raw iCal bytes into [`CalendarEvents`], evaluating windows relative
/// to `now` (a parameter so tests can pin the clock).
fn parse_events(content: &[u8], now: DateTime<Utc>) -> Result<CalendarEvents, CalendarError> {
    let parser = IcalParser::new(BufReader::new(content));

    // Collect all events first: override instances (RECURRENCE-ID) must be
    // known before their master's rule is expanded, wherever they appear.
    let mut all_events: Vec<IcalEvent> = Vec::new();
    for calendar in parser {
        match calendar {
            Ok(cal) => all_events.extend(cal.events),
            Err(e) => return Err(CalendarError::InvalidFormat(e.to_string())),
        }
    }

    // Occurrences superseded by an override instance, keyed by the master's
    // UID: the rule still generates them, but the override is the truth
    // (it is also in `all_events`, so it competes as a candidate itself).
    let mut overridden: HashMap<String, Vec<DateTime<Utc>>> = HashMap::new();
    for event in &all_events {
        if let (Some(uid), Some(recurrence_id)) = (
            get_property(event, "UID"),
            extract_datetime_property(event, "RECURRENCE-ID"),
        ) {
            overridden.entry(uid).or_default().push(recurrence_id);
        }
    }

    let mut events = CalendarEvents::default();
    for event in &all_events {
        // Cancelled events (and cancelled single occurrences) stay in the feed
        if get_property(event, "STATUS").as_deref() == Some("CANCELLED") {
            continue;
        }
        let Some(video_link) = get_video_link(event) else {
            continue;
        };
        for start_time in occurrences(event, now, &overridden) {
            // positive = the occurrence started that long ago
            let age = now.signed_duration_since(start_time);
            if age.num_minutes() > IN_PROGRESS_MAX_AGE_MINUTES {
                continue;
            }
            let candidate = NextEvent {
                start_time,
                summary: get_event_summary(event).unwrap_or_else(|| "Unknown".to_string()),
                video_link: video_link.clone(),
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
    Ok(events)
}

/// The concrete start times of an event that could matter now: the single
/// DTSTART for a one-off event, or the expanded occurrences (from an hour ago
/// onwards) of a recurring one.
fn occurrences(
    event: &IcalEvent,
    now: DateTime<Utc>,
    overridden: &HashMap<String, Vec<DateTime<Utc>>>,
) -> Vec<DateTime<Utc>> {
    if get_property(event, "RRULE").is_none() {
        return extract_datetime(event).into_iter().collect();
    }
    let superseded = get_property(event, "UID")
        .and_then(|uid| overridden.get(&uid).cloned())
        .unwrap_or_default();
    expand_rrule(event, now)
        .into_iter()
        .filter(|start| !superseded.contains(start))
        .collect()
}

/// Expands a recurring event's rule to concrete occurrences from an hour ago
/// onwards, limited to [`RECURRING_OCCURRENCE_LIMIT`]. EXDATEs are honoured
/// by the `rrule` crate; returns empty (with a warning) on unparseable rules.
fn expand_rrule(event: &IcalEvent, now: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    // The rrule crate parses raw iCalendar lines, so reconstruct the
    // recurrence-related lines of this event.
    let source = event
        .properties
        .iter()
        .filter(|p| ["DTSTART", "RRULE", "EXDATE", "RDATE"].contains(&p.name.as_str()))
        .map(property_line)
        .collect::<Vec<_>>()
        .join("\n");

    let rrule_set: rrule::RRuleSet = match source.parse() {
        Ok(set) => set,
        Err(e) => {
            warn!(
                "ignoring unparseable recurrence for {:?}: {e}",
                get_event_summary(event)
            );
            return Vec::new();
        }
    };

    let window_start = (now - TimeDelta::minutes(IN_PROGRESS_MAX_AGE_MINUTES)).with_timezone(&rrule::Tz::UTC);
    let result = rrule_set.after(window_start).all(RECURRING_OCCURRENCE_LIMIT);
    result.dates.into_iter().map(|d| d.with_timezone(&Utc)).collect()
}

/// Reconstructs a property's raw iCalendar line (`NAME;PARAM=VAL:VALUE`); the
/// `rrule` crate consumes raw lines rather than pre-parsed properties.
fn property_line(prop: &Property) -> String {
    let mut line = prop.name.clone();
    for (key, values) in prop.params.iter().flatten() {
        line.push(';');
        line.push_str(key);
        line.push('=');
        line.push_str(&values.join(","));
    }
    line.push(':');
    if let Some(ref value) = prop.value {
        line.push_str(value);
    }
    line
}

/// The event's literal start time (DTSTART), ignoring any recurrence rule.
fn extract_datetime(event: &IcalEvent) -> Option<DateTime<Utc>> {
    extract_datetime_property(event, "DTSTART")
}

/// Parses a datetime property (DTSTART, RECURRENCE-ID, ...) into UTC,
/// handling `TZID=` parameters, `...Z` UTC values and date-only values.
fn extract_datetime_property(event: &IcalEvent, name: &str) -> Option<DateTime<Utc>> {
    let dtstart_property = event.properties.iter().find(|p| p.name == name)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned "now" for deterministic window arithmetic: 09:08 UTC.
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 9, 9, 8, 0).unwrap()
    }

    fn feed(events: &str) -> String {
        format!("BEGIN:VCALENDAR\nVERSION:2.0\n{events}END:VCALENDAR\n")
    }

    fn parse(events: &str) -> CalendarEvents {
        parse_events(feed(events).as_bytes(), now()).unwrap()
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    const LINK: &str = "LOCATION:https://meet.google.com/abc-defg-hij\n";

    #[test]
    fn one_off_upcoming_event() {
        let events = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260709T100000Z\nSUMMARY:one-off\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(events.next.unwrap().start_time, utc(2026, 7, 9, 10, 0));
        assert!(events.in_progress.is_none());
    }

    #[test]
    fn event_without_video_link_ignored() {
        let events = parse("BEGIN:VEVENT\nDTSTART:20260709T100000Z\nSUMMARY:no link\nEND:VEVENT\n");
        assert!(events.next.is_none());
        assert!(events.in_progress.is_none());
    }

    #[test]
    fn recurring_event_started_8_minutes_ago() {
        // daily standup since July 1st; today's occurrence started at 09:00
        let events = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260701T090000Z\nRRULE:FREQ=DAILY\nSUMMARY:standup\nUID:a@x\n{LINK}END:VEVENT\n"
        ));
        let next = events.next.unwrap();
        assert_eq!(next.start_time, utc(2026, 7, 9, 9, 0));
        assert_eq!(next.summary, "standup");
        assert_eq!(events.in_progress.unwrap().start_time, utc(2026, 7, 9, 9, 0));
    }

    #[test]
    fn recurring_event_with_tzid() {
        // 11:00 Amsterdam == 09:00 UTC in July (CEST), weekdays only:
        // today's occurrence started 8 minutes before the pinned `now`
        let events = parse(&format!(
            "BEGIN:VEVENT\nDTSTART;TZID=Europe/Amsterdam:20260708T110000\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\nSUMMARY:eu standup\nUID:b@x\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(events.next.unwrap().start_time, utc(2026, 7, 9, 9, 0));
        assert_eq!(events.in_progress.unwrap().start_time, utc(2026, 7, 9, 9, 0));
    }

    #[test]
    fn old_recurring_event_out_of_both_windows() {
        // started 68 minutes ago: too old for either window, so the next
        // occurrence (tomorrow) is surfaced instead
        let events = parse(&format!(
            "BEGIN:VEVENT\nDTSTART;TZID=Europe/Amsterdam:20260708T100000\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\nSUMMARY:eu standup\nUID:b@x\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(events.next.unwrap().start_time, utc(2026, 7, 10, 8, 0));
        assert!(events.in_progress.is_none());
    }

    #[test]
    fn exdate_skips_occurrence() {
        let events = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260701T090000Z\nRRULE:FREQ=DAILY\nEXDATE:20260709T090000Z\nSUMMARY:standup\nUID:c@x\n{LINK}END:VEVENT\n"
        ));
        // today's occurrence removed; next is tomorrow's
        assert_eq!(events.next.unwrap().start_time, utc(2026, 7, 10, 9, 0));
        assert!(events.in_progress.is_none());
    }

    #[test]
    fn overridden_occurrence_replaced_by_instance() {
        // today's 09:00 occurrence was moved to 14:00 via an override instance
        let events = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260701T090000Z\nRRULE:FREQ=DAILY\nSUMMARY:standup\nUID:d@x\n{LINK}END:VEVENT\n\
             BEGIN:VEVENT\nDTSTART:20260709T140000Z\nRECURRENCE-ID:20260709T090000Z\nSUMMARY:standup (moved)\nUID:d@x\n{LINK}END:VEVENT\n"
        ));
        // the generated 09:00 occurrence is suppressed; the moved instance wins
        assert_eq!(events.next.unwrap().start_time, utc(2026, 7, 9, 14, 0));
        assert!(events.in_progress.is_none());
    }

    #[test]
    fn cancelled_event_ignored() {
        let events = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260709T100000Z\nSTATUS:CANCELLED\nSUMMARY:cancelled\n{LINK}END:VEVENT\n"
        ));
        assert!(events.next.is_none());
    }

    #[test]
    fn in_progress_and_next_are_tracked_separately() {
        // meeting started 30 min ago plus one later today
        let events = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260709T083800Z\nSUMMARY:current\n{LINK}END:VEVENT\n\
             BEGIN:VEVENT\nDTSTART:20260709T150000Z\nSUMMARY:later\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(events.next.unwrap().summary, "later");
        assert_eq!(events.in_progress.unwrap().summary, "current");
    }
}

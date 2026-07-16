use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use ical::IcalParser;
use ical::parser::ical::component::IcalEvent;
use ical::property::Property;
use std::collections::HashMap;
use std::fmt;
use std::io::BufReader;
use std::str::FromStr;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// A concrete occurrence of a calendar event with a video link.
#[derive(Debug, Clone, PartialEq)]
pub struct NextEvent {
    pub start_time: DateTime<Utc>,
    pub summary: String,
    pub video_link: String,
}

/// What the rest of the app needs from the calendar right now.
#[derive(Debug, Clone, Default)]
pub struct Cal {
    /// Earliest event that is upcoming or started <10 min ago; drives the
    /// countdown, the status line and the alerts.
    pub next_call: Option<NextEvent>,
}

/// Human-readable one-liner for the log, e.g. `next call "standup" at 2026-07-09T09:00Z`.
impl fmt::Display for Cal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.next_call {
            Some(event) => write!(
                f,
                "next call {:?} at {}",
                event.summary,
                event.start_time.format("%Y-%m-%dT%H:%MZ")
            ),
            None => write!(f, "no upcoming call"),
        }
    }
}

/// Why a calendar fetch failed; [`CalendarFeed::get`] returns this alongside
/// the (stale) calendar state so the caller can surface it.
#[derive(Debug)]
pub enum CalendarError {
    // for any status code > 400, should be `{status}: {text}`
    HttpStatus(String),
    // invalid ics file,
    InvalidFormat(String),
    // Other network errors
    NetworkError(String),
}

impl CalendarError {
    /// Short human-readable category, used as the notification subtitle.
    pub fn subtitle(&self) -> &'static str {
        match self {
            Self::HttpStatus(_) => "HTTP error fetching calendar",
            Self::InvalidFormat(_) => "Invalid ical response",
            Self::NetworkError(_) => "Network error fetching calendar",
        }
    }
}

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HttpStatus(err) | Self::InvalidFormat(err) | Self::NetworkError(err) => write!(f, "{err}"),
        }
    }
}

/// Alert window: events that started less than this many minutes ago still
/// count as `next_call`, so alerts can fire (one per minute since start).
pub const NEXT_MAX_AGE_MINUTES: i64 = 10;

/// Parse-time lookback: occurrences up to this old are kept as candidates.
/// Generously exceeds [`NEXT_MAX_AGE_MINUTES`] plus the worst-case cache age,
/// so per-tick selection never misses a recently started event.
const LOOKBACK_MINUTES: i64 = 60;

/// How many upcoming occurrences of a recurring event to consider. 2 covers
/// the "one in progress + the next one upcoming" case (e.g. a daily standup).
const RECURRING_OCCURRENCE_LIMIT: u16 = 2;

/// Cache TTL while `next_call` starts (or started) within 10 minutes: refetch
/// often enough to catch a meeting moved or cancelled at the last minute.
const NEAR_EVENT_TTL: Duration = Duration::from_secs(60);

/// Cache TTL otherwise; also how often a fetch failure is re-reported.
const IDLE_TTL: Duration = Duration::from_secs(180);

/// Slack on the expiry check: a cache expiring within this margin of a fetch
/// attempt is refreshed now rather than a whole sleep later. Without it the
/// TTL phase-slips against the tick cadence (expiry lands seconds after an
/// attempt) and the effective fetch interval doubles. Covers main's
/// FETCH_LEAD plus fetch latency and scheduling jitter.
const EXPIRY_SLACK: Duration = Duration::from_secs(15);

/// The calendar feed: owns the URL and an internal cache of expanded event
/// occurrences, so the main loop can call [`CalendarFeed::fetch`] every tick
/// (possibly every few seconds) while the network is hit at most once per TTL.
pub struct CalendarFeed {
    url: String,
    /// Expanded occurrences from the last successful fetch.
    candidates: Vec<NextEvent>,
    /// When the cache expires and the next `get` fetches again.
    expires: Instant,
}

impl CalendarFeed {
    /// A feed whose cache is empty and already expired: the first [`Self::fetch`] fetches.
    pub fn new(url: String) -> Self {
        Self {
            url,
            candidates: Vec::new(),
            expires: Instant::now(),
        }
    }

    /// Refreshes the cache if it has expired, returning any fetch error. On
    /// failure the stale candidates are kept; the expiry is bumped either
    /// way, so a persistent outage surfaces one error per TTL rather than one
    /// per tick. Read the resulting state with [`Self::cal`].
    pub fn fetch(&mut self, now: DateTime<Utc>) -> Option<CalendarError> {
        let fetch_start = Instant::now();
        let should_fetch = fetch_start + EXPIRY_SLACK >= self.expires;
        if should_fetch {
            let mut fetch_error = None;
            match fetch_candidates(&self.url, now) {
                Ok(candidates) => self.candidates = candidates,
                Err(e) => fetch_error = Some(e),
            }
            let cal = self.cal(now);
            let near_event = cal
                .next_call
                .as_ref()
                .is_some_and(|e| e.start_time.signed_duration_since(now) < TimeDelta::minutes(NEXT_MAX_AGE_MINUTES));
            self.expires = Instant::now() + if near_event { NEAR_EVENT_TTL } else { IDLE_TTL };
            if fetch_error.is_none() {
                info!("fetched calendar in {:.2}s, {cal}", fetch_start.elapsed().as_secs_f64());
            }
            fetch_error
        } else {
            None
        }
    }

    /// Pure window selection: `next_call` is the earliest candidate that is
    /// upcoming or started within the last [`NEXT_MAX_AGE_MINUTES`].
    pub fn cal(&self, now: DateTime<Utc>) -> Cal {
        let next_call = self
            .candidates
            .iter()
            // positive duration = the candidate started that long ago
            .filter(|c| now.signed_duration_since(c.start_time).num_minutes() <= NEXT_MAX_AGE_MINUTES)
            .min_by_key(|c| c.start_time)
            .cloned();
        Cal { next_call }
    }
}

/// Downloads the feed and expands it into candidate occurrences.
fn fetch_candidates(url: &str, now: DateTime<Utc>) -> Result<Vec<NextEvent>, CalendarError> {
    let response = reqwest::blocking::get(url).map_err(|e| CalendarError::NetworkError(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let status_text = response.text().unwrap_or_default();
        return Err(CalendarError::HttpStatus(format!("{status}: {status_text}",)));
    }

    let content = response
        .bytes()
        .map_err(|e| CalendarError::NetworkError(e.to_string()))?;

    parse_candidates(content.as_ref(), now)
}

/// Parses raw iCal bytes into candidate occurrences: every event occurrence
/// with a video link from `now - 60min` onward (RRULE-expanded, with
/// overridden and cancelled instances removed). No window selection here -
/// that happens per tick in [`CalendarFeed::cal`].
fn parse_candidates(content: &[u8], now: DateTime<Utc>) -> Result<Vec<NextEvent>, CalendarError> {
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

    let mut candidates = Vec::new();
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
            if now.signed_duration_since(start_time).num_minutes() > LOOKBACK_MINUTES {
                continue;
            }
            candidates.push(NextEvent {
                start_time,
                summary: get_event_summary(event).unwrap_or_else(|| "Unknown".to_string()),
                video_link: video_link.clone(),
            });
        }
    }
    Ok(candidates)
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

    let window_start = (now - TimeDelta::minutes(LOOKBACK_MINUTES)).with_timezone(&rrule::Tz::UTC);
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

fn get_property(event: &IcalEvent, name: &str) -> Option<String> {
    event
        .properties
        .iter()
        .find(|p| p.name == name)
        .and_then(|p| p.value.as_deref())
        .map(unescape_text)
}

/// Undo RFC 5545 TEXT escaping (`\,` `\;` `\\` `\n`/`\N`), which the `ical`
/// crate leaves in place - otherwise summaries render as "Bill\, Samuel".
/// Safe on URI-valued properties too: URIs never contain backslashes.
fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                // covers `\,` `\;` `\\`; an invalid escape keeps the char
                Some(escaped) => out.push(escaped),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn get_event_summary(event: &IcalEvent) -> Option<String> {
    get_property(event, "SUMMARY")
}

fn get_video_link(event: &IcalEvent) -> Option<String> {
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

    fn parse(events: &str) -> Cal {
        let calendar_feed = CalendarFeed {
            url: String::new(),
            candidates: parse_candidates(feed(events).as_bytes(), now()).unwrap(),
            expires: Instant::now(),
        };
        calendar_feed.cal(now())
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    const LINK: &str = "LOCATION:https://meet.google.com/abc-defg-hij\n";

    #[test]
    fn one_off_upcoming_event() {
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260709T100000Z\nSUMMARY:one-off\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(cal.next_call.unwrap().start_time, utc(2026, 7, 9, 10, 0));
    }

    #[test]
    fn summary_unescapes_rfc5545_text() {
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260709T100000Z\nSUMMARY:Bill\\, Samuel \\; co\\\\ two\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(cal.next_call.unwrap().summary, r"Bill, Samuel ; co\ two");
    }

    #[test]
    fn event_without_video_link_ignored() {
        let cal = parse("BEGIN:VEVENT\nDTSTART:20260709T100000Z\nSUMMARY:no link\nEND:VEVENT\n");
        assert!(cal.next_call.is_none());
    }

    #[test]
    fn recurring_event_started_8_minutes_ago() {
        // daily standup since July 1st; today's occurrence started at 09:00
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260701T090000Z\nRRULE:FREQ=DAILY\nSUMMARY:standup\nUID:a@x\n{LINK}END:VEVENT\n"
        ));
        let next = cal.next_call.unwrap();
        assert_eq!(next.start_time, utc(2026, 7, 9, 9, 0));
        assert_eq!(next.summary, "standup");
    }

    #[test]
    fn recurring_event_with_tzid() {
        // 11:00 Amsterdam == 09:00 UTC in July (CEST), weekdays only:
        // today's occurrence started 8 minutes before the pinned `now`
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART;TZID=Europe/Amsterdam:20260708T110000\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\nSUMMARY:eu standup\nUID:b@x\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(cal.next_call.unwrap().start_time, utc(2026, 7, 9, 9, 0));
    }

    #[test]
    fn old_recurring_occurrence_skipped() {
        // started 68 minutes ago: too old for the next_call window, so the
        // next occurrence (tomorrow) is surfaced instead
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART;TZID=Europe/Amsterdam:20260708T100000\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\nSUMMARY:eu standup\nUID:b@x\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(cal.next_call.unwrap().start_time, utc(2026, 7, 10, 8, 0));
    }

    #[test]
    fn exdate_skips_occurrence() {
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260701T090000Z\nRRULE:FREQ=DAILY\nEXDATE:20260709T090000Z\nSUMMARY:standup\nUID:c@x\n{LINK}END:VEVENT\n"
        ));
        // today's occurrence removed; next is tomorrow's
        assert_eq!(cal.next_call.unwrap().start_time, utc(2026, 7, 10, 9, 0));
    }

    #[test]
    fn overridden_occurrence_replaced_by_instance() {
        // today's 09:00 occurrence was moved to 14:00 via an override instance
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260701T090000Z\nRRULE:FREQ=DAILY\nSUMMARY:standup\nUID:d@x\n{LINK}END:VEVENT\n\
             BEGIN:VEVENT\nDTSTART:20260709T140000Z\nRECURRENCE-ID:20260709T090000Z\nSUMMARY:standup (moved)\nUID:d@x\n{LINK}END:VEVENT\n"
        ));
        // the generated 09:00 occurrence is suppressed; the moved instance wins
        assert_eq!(cal.next_call.unwrap().start_time, utc(2026, 7, 9, 14, 0));
    }

    #[test]
    fn cancelled_event_ignored() {
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260709T100000Z\nSTATUS:CANCELLED\nSUMMARY:cancelled\n{LINK}END:VEVENT\n"
        ));
        assert!(cal.next_call.is_none());
    }

    #[test]
    fn started_event_ages_out_in_favour_of_later_one() {
        // meeting started 30 min ago plus one later today
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260709T083800Z\nSUMMARY:current\n{LINK}END:VEVENT\n\
             BEGIN:VEVENT\nDTSTART:20260709T150000Z\nSUMMARY:later\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(cal.next_call.unwrap().summary, "later");
    }

    #[test]
    fn next_call_window_boundary() {
        // started exactly 10 minutes ago: still next_call
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260709T085800Z\nSUMMARY:ten\n{LINK}END:VEVENT\n"
        ));
        assert_eq!(cal.next_call.unwrap().summary, "ten");

        // started 11 minutes ago: no longer next_call
        let cal = parse(&format!(
            "BEGIN:VEVENT\nDTSTART:20260709T085700Z\nSUMMARY:eleven\n{LINK}END:VEVENT\n"
        ));
        assert!(cal.next_call.is_none());
    }
}

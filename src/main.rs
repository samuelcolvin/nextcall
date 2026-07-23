mod camera;
mod config;
mod ical;
mod logic;
mod notifications;
mod say;
mod tray;

use anyhow::Result as AnyhowResult;
use chrono::{DateTime, TimeDelta, Utc};
use std::fs::OpenOptions;
use std::thread::sleep;
use tracing::{error, info};
use tracing_subscriber::fmt::time::ChronoLocal;

/// Log timestamp format: RFC 3339 local time at whole-second precision —
/// chrono has no 1-digit fraction specifier, and microseconds are noise here.
const LOG_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%:z";

/// Sets up tracing to a per-process temp file and stderr, returning the log
/// path so the tray menu's "View Log" item can open it. A fresh file per run
/// (pid in the name) means old logs never get clobbered mid-read.
fn init_logging() -> AnyhowResult<String> {
    let log_path = std::env::temp_dir()
        .join(format!("nextcall-{}.log", std::process::id()))
        .to_string_lossy()
        .into_owned();

    // Create the file up front so "View Log" works before the first entry
    let _log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)?;

    // Set up tracing subscriber with both file and stderr output
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let writer_path = log_path.clone();
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(move || {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&writer_path)
                .expect("Failed to open log file")
        })
        .with_timer(ChronoLocal::new(LOG_TIME_FORMAT.to_owned()))
        .with_ansi(false)
        .with_target(false);

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_timer(ChronoLocal::new(LOG_TIME_FORMAT.to_owned()))
        .with_ansi(true)
        .with_target(false);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .with(LevelFilter::INFO)
        .init();

    Ok(log_path)
}

/// Logs a fatal error, surfaces it as a notification, and exits.
fn fatal(subtitle: &str, message: &str) -> ! {
    error!("Fatal error: {message}");
    notifications::send("Nextcall Configuration", Some(subtitle), message, None);
    std::process::exit(1);
}

fn main() {
    match init_logging() {
        Ok(log_path) => tray::set_log_path(&log_path),
        Err(e) => eprintln!("Failed to initialize logging: {}", e),
    }

    info!("Nextcall starting up");

    notifications::startup();

    let config = match config::get_config() {
        Ok(Some(config)) => config,
        Ok(None) => {
            fatal(
                "WARNING: nextcall.toml not found",
                "Create ~/nextcall.toml to configure Nextcall",
            );
        }
        Err(err) => fatal("ERROR", &err.to_string()),
    };

    info!("Configuration loaded: {config}");

    // Calendar polling and alerting run off the main thread so the AppKit run
    // loop below is never blocked by network requests.
    std::thread::spawn(move || {
        if let Err(err) = background(config) {
            fatal("ERROR", &err.to_string());
        }
    });

    // Blocks forever running the menu bar app; "Quit" terminates the process.
    tray::run()
}

/// How long before each scheduled tick the calendar is fetched, so network
/// latency never delays an alert firing at its exact instant.
const FETCH_LEAD: TimeDelta = TimeDelta::seconds(20);

/// The main loop: almost stateless. Each tick asks the feed for the calendar
/// (cached, network at most once per TTL), reads the camera and the tray's
/// dismiss toggle, lets the pure [`logic::step`] decide display/alert/sleep,
/// applies the side effects, and sleeps. The only state: the feed's cache,
/// the previous tick's timestamp (alerts exactly-once) and a log-only var.
fn background(config: config::Config) -> AnyhowResult<()> {
    let mut feed = ical::CalendarFeed::new(config.ical_url);
    let mut prev_tick = Utc::now();
    let mut scheduled = Utc::now();
    // Previous tick's dismissal, kept only to log transitions.
    let mut prev_dismissed: Option<DateTime<Utc>> = None;

    loop {
        // warm the cache ~FETCH_LEAD before the scheduled tick so network
        // latency never delays it (usually a no-op cache hit)
        // failures surface as the tray's warning icon (self-clearing on the
        // next successful fetch) plus a log entry — not a notification
        let fetch_error = feed.fetch(Utc::now());
        tray::set_warning(fetch_error.is_some());
        if let Some(err) = fetch_error {
            error!("{}: {err}", err.subtitle());
        }
        sleep_until(scheduled);

        let now = Utc::now();
        // cache hit: re-selects the calendar window at the tick itself, so
        // next_call isn't up to a sleep-length stale
        let cal = feed.cal(now);
        // the tray owns the dismiss toggle; read it like the camera state and
        // match against the call that is still next - a stale value (the call
        // changed while we slept) must never mute a different call
        let dismissed = tray::dismissed_ts().and_then(|ts| {
            let event = cal.next_call.as_ref()?;
            (event.start_time.timestamp() == ts).then_some(event.start_time)
        });
        if dismissed != prev_dismissed {
            info!("dismissed call: {dismissed:?}");
            prev_dismissed = dismissed;
        }
        let camera_active = camera::camera_active();
        let step = logic::step(&cal, now, prev_tick, camera_active, dismissed);
        tray::set_status(&step.status);
        tray::set_title(&step.title);
        // arm the menu's Dismiss item with the call it would act on
        // (0 disables it: no upcoming call) and expire a stale dismissal
        tray::set_dismiss_target(cal.next_call.as_ref().map_or(0, |e| e.start_time.timestamp()));
        if let Some((event, minutes)) = step.alert {
            logic::fire_alert(&event, minutes, camera_active, config.eleven_labs_key.as_deref());
        }

        prev_tick = now;
        scheduled = now + TimeDelta::from_std(step.sleep)?;
        info!(
            "sleeping {:.2}s until {}",
            step.sleep.as_secs_f64(),
            scheduled.format("%H:%M:%S")
        );
        // long leg of the sleep; zero when the next tick is <= FETCH_LEAD away
        sleep_until(scheduled - FETCH_LEAD);
    }
}

/// Sleeps until the wall-clock instant `t` (no-op if already past). Wall time
/// rather than `Instant`: `Instant` doesn't advance during system sleep, and
/// alert firing can block for seconds; recomputing keeps ticks on schedule.
fn sleep_until(t: DateTime<Utc>) {
    if let Ok(duration) = t.signed_duration_since(Utc::now()).to_std() {
        sleep(duration.min(logic::DEFAULT_CHECK_INTERVAL));
    }
}

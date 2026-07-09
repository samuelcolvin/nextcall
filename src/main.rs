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
use tracing_subscriber::fmt::time::LocalTime;

/// Sets up tracing to both `~/nextcall.log` (truncated on startup) and stderr.
fn init_logging() -> AnyhowResult<()> {
    // Expand home directory
    let home = config::home()?;
    let log_path = format!("{}/nextcall.log", home);

    // Truncate/create the log file
    let _log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)?;

    // Set up tracing subscriber with both file and stderr output
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(move || {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .expect("Failed to open log file")
        })
        .with_timer(LocalTime::rfc_3339())
        .with_ansi(false)
        .with_target(false);

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_timer(LocalTime::rfc_3339())
        .with_ansi(true)
        .with_target(false);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .with(LevelFilter::INFO)
        .init();

    Ok(())
}

/// Logs a fatal error, surfaces it as a notification, and exits.
fn fatal(subtitle: &str, message: &str) -> ! {
    error!("Fatal error: {message}");
    notifications::send("NextCall Configuration", Some(subtitle), message, None);
    std::process::exit(1);
}

fn main() {
    if let Err(e) = init_logging() {
        eprintln!("Failed to initialize logging: {}", e);
    }

    info!("NextCall starting up");

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

    info!("Configuration loaded successfully from {:?}", config);

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
const FETCH_LEAD: TimeDelta = TimeDelta::seconds(10);

/// The main loop: almost stateless. Each tick asks the feed for the calendar
/// (cached, network at most once per TTL), lets the pure [`logic::step`]
/// decide display/alert/sleep from `(cal, now, prev_tick, camera)`, applies
/// the side effects, and sleeps. The only state is the feed's cache and the
/// previous tick's timestamp (which makes alerts exactly-once).
fn background(config: config::Config) -> AnyhowResult<()> {
    let mut feed = ical::CalendarFeed::new(config.ical_url);
    let mut prev_tick = Utc::now();
    let mut scheduled = Utc::now();

    loop {
        // warm the cache ~FETCH_LEAD before the scheduled tick so network
        // latency never delays it (usually a no-op cache hit)
        let fetch_error = feed.fetch(Utc::now());
        if let Some(err) = fetch_error {
            error!("Error fetching calendar: {err}");
            notifications::send("Next Call", Some(err.subtitle()), &err.to_string(), None);
        }
        sleep_until(scheduled);

        let now = Utc::now();
        // cache hit: re-selects the calendar windows at the tick itself, so
        // in_call/next_call aren't up to a sleep-length stale
        let cal = feed.cal(now);
        let camera_active = camera::camera_active();
        let step = logic::step(&cal, now, prev_tick, camera_active);
        tray::set_status(&step.status);
        step.icon.show();
        if let Some((event, minutes)) = step.alert {
            logic::fire_alert(&event, minutes, camera_active, config.eleven_labs_key.as_deref());
        }

        prev_tick = now;
        scheduled = now + TimeDelta::from_std(step.sleep)?;
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

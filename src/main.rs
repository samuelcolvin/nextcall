mod camera;
mod config;
mod ical;
mod logic;
mod notifications;
mod say;
mod tray;

use anyhow::Result as AnyhowResult;
use chrono::{Timelike, Utc};
use std::fs::OpenOptions;
use std::thread::sleep;
use std::time::{Duration, Instant};
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

/// Endless loop: find the relevant events, update the tray, sleep until the
/// next interesting moment, and run the alert sequence when a call starts.
/// Whenever some event is in progress, the sleeps watch the camera instead,
/// showing a person icon in the tray while the user is on the call.
fn background(config: config::Config) -> AnyhowResult<()> {
    // True until the first fetch has been handled: an event already in
    // progress at launch is shown in the tray but its alerts are not replayed.
    let mut first_run = true;
    let mut events = ical::CalendarEvents::default();
    // Event whose alert sequence has already run. A started event stays in the
    // `next` window for 10 minutes, so without this the sequence would restart
    // (and re-notify) on every fetch until the event ages out.
    let mut alerted: Option<ical::NextEvent> = None;

    loop {
        events = logic::find_events(&config.ical_url, events);
        tray::set_status(&logic::status_line(&events));

        // What to show while sleeping, and for how long, based on `next`.
        let (icon_text, next) = match events.next {
            Some(ref next_event) => {
                let result = logic::calc_sleep(next_event)?;
                (result.icon_text, result.next)
            }
            None => ("...".into(), logic::StepNext::Sleep(logic::DEFAULT_CHECK_INTERVAL)),
        };

        match next {
            logic::StepNext::Sleep(duration) => {
                if events.in_progress.is_some() {
                    // A call may be live: person icon while the camera is on.
                    logic::watch_camera_until(Instant::now() + duration, &icon_text);
                } else {
                    tray::set_title(&icon_text);
                    sleep(duration);
                }
            }
            logic::StepNext::EventStarted(event) => {
                if first_run {
                    // Restarted mid-meeting: skip straight to watching.
                    alerted = Some(event.clone());
                }
                if alerted.as_ref() == Some(&event) {
                    // Already alerted: watch the camera until the next minute
                    // boundary (when the countdown ticks and the calendar is
                    // refetched).
                    let deadline = Instant::now() + Duration::from_secs(60 - u64::from(Utc::now().second()));
                    logic::watch_camera_until(deadline, &icon_text);
                } else {
                    tray::set_title(&icon_text);
                    logic::event_started(event.clone(), config.eleven_labs_key.as_deref())?;
                    alerted = Some(event);
                }
            }
        };
        first_run = false;
    }
}

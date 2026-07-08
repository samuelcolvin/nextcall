mod camera;
mod config;
mod ical;
mod logic;
mod notifications;
mod say;
mod tray;

use anyhow::Result as AnyhowResult;
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

/// Endless loop: find the next event, update the tray countdown, sleep until
/// the next interesting moment, and run the alert sequence when a call starts.
fn background(config: config::Config) -> AnyhowResult<()> {
    let mut first_run = true;
    let mut next_event_opt: Option<ical::NextEvent> = None;

    loop {
        next_event_opt = logic::find_next_event(&config.ical_url, first_run, next_event_opt);
        first_run = false;

        let Some(ref next_event) = next_event_opt else {
            continue;
        };

        let result = logic::calc_sleep(next_event)?;
        tray::set_title(&result.icon_text);

        match result.next {
            logic::StepNext::Sleep(duration) => {
                sleep(duration);
            }
            logic::StepNext::EventStarted(event) => {
                logic::event_started(event, config.eleven_labs_key.as_deref())?;
            }
        };
    }
}

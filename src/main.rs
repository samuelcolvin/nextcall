mod camera;
mod config;
mod ical;
mod icon;
mod logic;
mod notifications;
mod say;

use anyhow::Result as AnyhowResult;
use std::borrow::Cow;
use std::fs::OpenOptions;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use tracing::{error, info};
use tracing_subscriber::fmt::time::LocalTime;
use tray_icon::{
    TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};
use winit::event_loop::{ControlFlow, EventLoopBuilder};

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

fn main() {
    if let Err(e) = init_logging() {
        eprintln!("Failed to initialize logging: {}", e);
    }

    info!("NextCall starting up");

    notifications::startup();
    if let Err(err) = run_ui() {
        error!("Fatal error: {}", err);
        notifications::send("NextCall Configuration", Some("ERROR"), &err.to_string(), None);
        std::process::exit(1);
    }
}

fn run_ui() -> AnyhowResult<()> {
    let event_loop = EventLoopBuilder::new().build()?;

    let icon = icon::create_icon_infinity();

    let menu = Menu::new();

    let quit_item = MenuItem::new("Quit Nextcall", true, None);
    menu.append(&quit_item)?;

    // Create tray icon
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon.clone())
        .build()?;

    let menu_channel = MenuEvent::receiver();

    let Some(config) = config::get_config()? else {
        error!("Configuration file nextcall.toml not found in current directory or home directory");
        notifications::send(
            "NextCall Configuration",
            Some("WARNING: nextcall.toml not found"),
            "Create ~/nextcall.toml to configure Nextcall",
            None,
        );
        std::process::exit(1);
    };

    info!("Configuration loaded successfully from {:?}", config);

    // Channel for receiving icon updates from background thread
    let (icon_tx, icon_rx) = mpsc::channel::<Cow<'static, str>>();

    let icon_tx_clone = icon_tx.clone();
    std::thread::spawn(move || {
        background(config, icon_tx_clone).unwrap();
    });

    // Run event loop
    event_loop.run(move |_event, event_loop_window_target| {
        // Use dynamic check interval based on upcoming events
        event_loop_window_target.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_secs(10)));

        // Check for menu events
        if let Ok(event) = menu_channel.try_recv() {
            if event.id == quit_item.id() {
                event_loop_window_target.exit();
            }
        }

        // Check for icon updates from background thread
        if let Ok(new_icon) = icon_rx.try_recv() {
            println!("got new icon {:?}", new_icon);
            tray_icon
                .set_icon(Some(icon::create_icon_with_text(&new_icon)))
                .unwrap();
        }
    })?;
    Ok(())
}

fn background(config: config::Config, icon_tx: Sender<Cow<'static, str>>) -> AnyhowResult<()> {
    loop {
        let result = logic::find_next_event(&config.ical_url)?;

        let _ = icon_tx.send(result.icon_text);

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

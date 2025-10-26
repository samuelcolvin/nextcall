mod camera;
mod config;
mod ical;
mod icon;
mod logic;
mod notifications;
mod say;

use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;
use tray_icon::{
    TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};
use winit::event_loop::{ControlFlow, EventLoopBuilder};

fn main() {
    // Create event loop
    let event_loop = EventLoopBuilder::new().build().unwrap();
    notifications::startup().unwrap();

    let icon = icon::create_icon_infinity();

    let menu = Menu::new();

    let about = MenuItem::new("NextCall", true, None);
    menu.append(&about).unwrap();

    let quit_item = MenuItem::new("Quit", true, None);
    menu.append(&quit_item).unwrap();

    // Create tray icon
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon.clone())
        .build()
        .unwrap();

    let menu_channel = MenuEvent::receiver();

    let Some(config) = config::get_config().unwrap() else {
        notifications::send(
            "NextCall Configuration",
            Some("WARNING: nextcall.toml not found"),
            "Create ~/nextcall.toml to configure Nextcall",
            None,
        );
        std::process::exit(1);
    };

    // Track last update time and active event
    let mut last_update: Option<Instant> = None;
    let mut active_event: Option<logic::ActiveEvent> = None;
    let mut check_interval = Duration::from_secs(180); // Default: 3 minutes

    // Channel for receiving icon updates from background thread
    let (icon_tx, icon_rx) = mpsc::channel::<Option<tray_icon::Icon>>();
    // Channel for receiving active event updates from background thread
    let (event_tx, event_rx) = mpsc::channel::<Option<logic::ActiveEvent>>();
    // Channel for receiving next check duration from background thread
    let (duration_tx, duration_rx) = mpsc::channel::<Duration>();

    // Run event loop
    event_loop
        .run(move |_event, event_loop_window_target| {
            // Use dynamic check interval based on upcoming events
            event_loop_window_target.set_control_flow(ControlFlow::WaitUntil(Instant::now() + check_interval));

            // Check for menu events
            if let Ok(event) = menu_channel.try_recv() {
                if event.id == quit_item.id() {
                    event_loop_window_target.exit();
                }
            }

            // Check for icon updates from background thread
            if let Ok(Some(new_icon)) = icon_rx.try_recv() {
                tray_icon.set_icon(Some(new_icon)).unwrap();
            }

            // Check for active event updates from background thread
            if let Ok(new_active) = event_rx.try_recv() {
                active_event = new_active;
            }

            // Check for duration updates from background thread
            if let Ok(new_duration) = duration_rx.try_recv() {
                check_interval = new_duration;
            }

            let elapsed = match last_update {
                Some(last_update) => last_update.elapsed(),
                None => Duration::from_secs(3600),
            };

            // Update based on dynamic check interval
            if elapsed >= check_interval {
                last_update = Some(Instant::now());
                // Spawn thread to run step() without blocking UI
                let ics_url = config.ical_url.clone();
                let eleven_labs_key = config.eleven_labs_key.clone();
                let icon_tx_clone = icon_tx.clone();
                let event_tx_clone = event_tx.clone();
                let duration_tx_clone = duration_tx.clone();
                let mut current_active = active_event.clone();

                std::thread::spawn(move || {
                    if let Ok(result) = logic::step(&ics_url, eleven_labs_key.as_deref(), current_active.as_mut()) {
                        if let Some(icon) = result.new_icon {
                            let _ = icon_tx_clone.send(Some(icon));
                        }
                        // Send back the updated active event (or the new one if there is one)
                        if let Some(new_active) = result.new_active_event {
                            let _ = event_tx_clone.send(Some(new_active));
                        } else if let Some(active) = current_active {
                            // Send back the potentially updated current active event
                            let _ = event_tx_clone.send(Some(active));
                        }
                        // Send back the next check duration
                        let _ = duration_tx_clone.send(result.next_check_duration);
                    }
                });
            }
        })
        .unwrap();
}

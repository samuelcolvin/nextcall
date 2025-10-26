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

    let opt_config = config::get_config().unwrap();
    if opt_config.is_none() {
        notifications::send("NextCall Configuration", None, "WARNING: nextcall.toml not found", None);
    }

    // Track last update time and active event
    let mut last_update: Option<Instant> = None;
    let mut active_event: Option<logic::ActiveEvent> = None;

    // Channel for receiving icon updates from background thread
    let (icon_tx, icon_rx) = mpsc::channel::<Option<tray_icon::Icon>>();
    // Channel for receiving active event updates from background thread
    let (event_tx, event_rx) = mpsc::channel::<Option<logic::ActiveEvent>>();

    // Run event loop
    event_loop
        .run(move |_event, event_loop_window_target| {
            // Poll more frequently (every 10 seconds) to catch events and reminders
            event_loop_window_target
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(10_000)));

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

            let elapsed = match last_update {
                Some(last_update) => last_update.elapsed(),
                None => Duration::from_secs(3600),
            };

            // Update every 10 seconds to catch events and reminders
            if elapsed >= Duration::from_secs(10) {
                last_update = Some(Instant::now());
                if let Some(config) = opt_config.as_ref() {
                    // Spawn thread to run step() without blocking UI
                    let ics_url = config.ical_url.clone();
                    let eleven_labs_key = config.eleven_labs_key.clone();
                    let icon_tx_clone = icon_tx.clone();
                    let event_tx_clone = event_tx.clone();
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
                        }
                    });
                }
            }
        })
        .unwrap();
}

mod camera;
mod config;
mod dialog;
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

    let configure = MenuItem::new("Configure", true, None);
    menu.append(&configure).unwrap();

    let quit_item = MenuItem::new("Quit", true, None);
    menu.append(&quit_item).unwrap();

    // Create tray icon
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon.clone())
        .build()
        .unwrap();

    let menu_channel = MenuEvent::receiver();

    let mut opt_ics_url = config::get_config().unwrap();
    if opt_ics_url.is_none() {
        notifications::send("NextCall Configuration", None, "WARNING: ICS URL not configured", None);
    }

    // Track last update time and icon state
    let mut last_update: Option<Instant> = None;

    // Channel for receiving icon updates from background thread
    let (icon_tx, icon_rx) = mpsc::channel::<Option<tray_icon::Icon>>();

    // Run event loop
    event_loop
        .run(move |_event, event_loop_window_target| {
            // Poll for new events
            event_loop_window_target
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(200)));

            // Check for menu events
            if let Ok(event) = menu_channel.try_recv() {
                if event.id == configure.id() {
                    if let Some(url) = dialog::show_url_input_dialog(opt_ics_url.as_deref()).unwrap() {
                        config::set_config(&url).unwrap();
                        opt_ics_url = Some(url);
                        last_update = None;
                    }
                } else if event.id == quit_item.id() {
                    event_loop_window_target.exit();
                }
            }

            // Check for icon updates from background thread
            if let Ok(opt_icon) = icon_rx.try_recv() {
                if let Some(new_icon) = opt_icon {
                    tray_icon.set_icon(Some(new_icon)).unwrap();
                }
            }

            let elapsed = match last_update {
                Some(last_update) => last_update.elapsed(),
                None => Duration::from_secs(3600),
            };
            if elapsed >= Duration::from_secs(60) {
                last_update = Some(Instant::now());
                if let Some(ics_url) = opt_ics_url.as_deref() {
                    // Spawn thread to run step() without blocking UI
                    let ics_url = ics_url.to_string();
                    let tx = icon_tx.clone();
                    std::thread::spawn(move || {
                        if let Ok(icon) = logic::step(&ics_url) {
                            let _ = tx.send(icon);
                        }
                    });
                }
            }
        })
        .unwrap();
}

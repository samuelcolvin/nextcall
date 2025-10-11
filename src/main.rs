mod camera;
mod config;
mod dialog;
mod ical;
mod icon;
mod notifications;
mod say;

use anyhow::Result as AnyhowResult;
use std::time::Instant;

use std::time::Duration;
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

    // Run event loop
    event_loop
        .run(move |_event, event_loop_window_target| {
            // Poll for new events
            event_loop_window_target
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(200)));

            // Check for menu events
            if let Ok(event) = menu_channel.try_recv() {
                if event.id == configure.id() {
                    if let Some(url) = dialog::show_url_input_dialog() {
                        config::set_config(&url).unwrap();
                        opt_ics_url = Some(url);
                        last_update = None;
                    }
                } else if event.id == quit_item.id() {
                    event_loop_window_target.exit();
                }
            }
            let elapsed = match last_update {
                Some(last_update) => last_update.elapsed(),
                None => Duration::from_secs(3600),
            };
            if elapsed >= Duration::from_secs(60) {
                last_update = Some(Instant::now());
                if let Some(ics_url) = opt_ics_url.as_deref() {
                    if let Some(new_icon) = step(ics_url).unwrap() {
                        tray_icon.set_icon(Some(new_icon)).unwrap();
                    }
                }
            }
        })
        .unwrap();
}

fn step(ics_url: &str) -> AnyhowResult<Option<tray_icon::Icon>> {
    let calendar = match ical::get_ics(&ics_url) {
        Ok(calendar) => calendar,
        Err(ical::CalendarError::HttpStatus(err)) => {
            notifications::send("Next Call", Some("Invalid URL"), &err, None);
            return Ok(None);
        }
        Err(ical::CalendarError::InvalidFormat(err)) => {
            notifications::send("Next Call", Some("Invalid ical response"), &err, None);
            return Ok(None);
        }
        Err(ical::CalendarError::NetworkError(err)) => {
            eprintln!("Network error: {}", err);
            return Ok(None);
        }
    };

    let new_icon = icon::create_icon_with_text("60", false);

    // say::say("Call with John Doe just started.").unwrap();
    notifications::send("Next Call", None, &format!("calendar: {calendar:?}"), None);

    if camera::camera_active() {
        println!("Camera is active");
    } else {
        println!("Camera is not active");
    }
    Ok(Some(new_icon))
}

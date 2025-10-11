mod camera;
mod config;
mod dialog;
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

    let mut ics_url = config::get_config().unwrap();
    if ics_url.is_none() {
        notifications::send(
            "NextCall Configuration",
            None,
            "WARNING: ICS URL not configured",
            None,
        );
    }

    // Track last update time and icon state
    let mut last_update = Instant::now();

    // Run event loop
    event_loop
        .run(move |_event, event_loop_window_target| {
            // Poll for new events
            event_loop_window_target.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(200),
            ));

            // Check for menu events
            if let Ok(event) = menu_channel.try_recv() {
                if event.id == configure.id() {
                    if let Some(url) = dialog::show_url_input_dialog() {
                        config::set_config(&url).unwrap();
                        ics_url = Some(url);
                    }
                } else if event.id == quit_item.id() {
                    event_loop_window_target.exit();
                }
            }

            if last_update.elapsed() >= Duration::from_secs(10) {
                last_update = Instant::now();
                if let Some(new_icon) = step(ics_url.as_deref()).unwrap() {
                    tray_icon.set_icon(Some(new_icon)).unwrap();
                }
            }
        })
        .unwrap();
}

fn step(ics_url: Option<&str>) -> AnyhowResult<Option<tray_icon::Icon>> {
    let new_icon = icon::create_icon_with_text("60", false);

    say::say("Call with John Doe just started.").unwrap();
    notifications::send("Next Call", None, &format!("URL: {ics_url:?}"), None);

    if camera::camera_active() {
        println!("Camera is active");
    } else {
        println!("Camera is not active");
    }
    Ok(Some(new_icon))
}

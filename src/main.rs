mod icon;

use std::time::Duration;
use tray_icon::{
    TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};
use winit::event_loop::{ControlFlow, EventLoopBuilder};

fn main() {
    // Create event loop
    let event_loop = EventLoopBuilder::new().build().unwrap();

    let icon = icon::create_icon_infinity();

    // Create menu
    let menu = Menu::new();
    let hello_item = MenuItem::new("Hello World", true, None);
    menu.append(&hello_item).unwrap();

    let open_link_item = MenuItem::new("Open example.com", true, None);
    menu.append(&open_link_item).unwrap();

    let quit_item = MenuItem::new("Quit", true, None);
    menu.append(&quit_item).unwrap();

    // Create tray icon
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Hello World App")
        .with_icon(icon)
        .build()
        .unwrap();

    println!("System tray app is running. Check your menu bar!");

    // Set up menu event handler
    let menu_channel = MenuEvent::receiver();

    // Track last update time and icon state
    let mut last_update = std::time::Instant::now();
    let mut icon_state = 0; // Cycle through different icon states

    // Run event loop
    event_loop
        .run(move |_event, event_loop_window_target| {
            // Poll for new events
            event_loop_window_target.set_control_flow(ControlFlow::WaitUntil(
                std::time::Instant::now() + Duration::from_millis(200),
            ));

            // Check for menu events
            if let Ok(event) = menu_channel.try_recv() {
                if event.id == hello_item.id() {
                    println!("Hello World menu item clicked!");
                } else if event.id == open_link_item.id() {
                    println!("Opening example.com...");
                    if let Err(e) = open::that("https://example.com") {
                        eprintln!("Failed to open URL: {}", e);
                    }
                } else if event.id == quit_item.id() {
                    println!("Quitting application...");
                    event_loop_window_target.exit();
                }
            }

            // Update icon every 5 seconds, cycling through different states
            if last_update.elapsed() >= Duration::from_secs(5) {
                let new_icon = match icon_state {
                    0 => icon::create_icon_with_text("60", false),
                    1 => icon::create_icon_with_text("5", false),
                    2 => icon::create_icon_with_text("0", false),
                    3 => icon::create_icon_with_text("-2!", false),
                    _ => icon::create_icon_infinity(),
                };
                tray_icon.set_icon(Some(new_icon)).ok();

                let display_text = match icon_state {
                    0 => "infinity (∞)",
                    1 => "60",
                    2 => "0",
                    3 => "-2!",
                    _ => "infinity",
                };

                icon_state = (icon_state + 1) % 4;
                last_update = std::time::Instant::now();
                println!("Updated icon to: {}", display_text);
            }
        })
        .unwrap();
}

use mac_notification_sys::*;

pub fn send_notification(
    title: &str,
    subtitle: Option<&str>,
    body: &str,
    url: &str,
) -> Result<(), String> {
    // Set the application bundle
    let bundle = get_bundle_identifier_or_default("com.apple.Terminal");
    set_application(&bundle).map_err(|e| format!("Failed to set application: {:?}", e))?;

    // Send a notification with sound and action button
    let response = mac_notification_sys::send_notification(
        title,
        subtitle,
        body,
        Some(
            &Notification::new()
                .sound("Blow")
                .main_button(MainButton::SingleAction("Open")),
        ),
    )
    .map_err(|e| format!("Failed to send notification: {:?}", e))?;

    // Handle the notification response
    match response {
        NotificationResponse::Click | NotificationResponse::ActionButton(_) => {
            if let Err(e) = open::that(url) {
                eprintln!("Failed to open URL: {}", e);
            }
        }
        NotificationResponse::CloseButton(_) | NotificationResponse::Reply(_) | NotificationResponse::None => {
            // Do nothing for these responses
        }
    }

    Ok(())
}

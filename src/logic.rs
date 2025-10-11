use crate::{camera, ical, icon, notifications, say};
use anyhow::Result as AnyhowResult;

pub fn step(ics_url: &str) -> AnyhowResult<Option<tray_icon::Icon>> {
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

    say::say("Call with John Doe just started.").unwrap();
    notifications::send("Next Call", None, &format!("calendar: {calendar:?}"), None);

    if camera::camera_active() {
        println!("Camera is active");
    } else {
        println!("Camera is not active");
    }
    Ok(Some(new_icon))
}

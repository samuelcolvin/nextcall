use anyhow::Result;
use std::process::Command;

pub fn show_url_input_dialog(current_url: Option<&str>) -> Result<Option<String>> {
    // Use AppleScript to show a native dialog that definitely supports pasting
    let script = format!(
        r#"
        display dialog "Please enter the URL to your ICS calendar file:" default answer "{}" with title "NextCall" buttons {{"Cancel", "Update"}} default button "Update"
        text returned of result
    "#,
        current_url.unwrap_or("")
    );

    let output = Command::new("osascript").arg("-e").arg(script).output()?;
    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !result.is_empty() {
            println!("Returning URL: {}", result);
            return Ok(Some(result));
        }
    }
    Ok(None)
}

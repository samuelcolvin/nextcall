use std::process::Command;

pub fn say(text: &str) -> Result<(), String> {
    Command::new("say")
        .arg("-v")
        .arg("Moira")
        .arg(text)
        .spawn()
        .map_err(|e| format!("Failed to spawn say command: {}", e))?;

    Ok(())
}

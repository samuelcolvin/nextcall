use anyhow::Result as AnyhowResult;
use rodio::OutputStreamBuilder;
use std::fs::File;
use std::io::BufReader;
use std::io::Write;
use std::process::Command;

pub fn say(text: &str, eleven_labs_api_key: Option<&str>) -> AnyhowResult<()> {
    if let Some(api_key) = eleven_labs_api_key {
        say_eleven_labs(text, api_key)
    } else {
        say_builtin(text)
    }
}

fn say_eleven_labs(text: &str, api_key: &str) -> AnyhowResult<()> {
    // Generate MP3 using ElevenLabs API
    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://api.elevenlabs.io/v1/text-to-speech/JBFqnCBsd6RMkjVDRZzb?output_format=mp3_44100_128")
        .header("xi-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "text": text,
            "model_id": "eleven_multilingual_v2"
        }))
        .send()?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Failed to generate audio: HTTP {}", response.status()));
    }

    // Write the MP3 content to file
    let audio_bytes = response.bytes()?;
    let mut file = File::create("file.mp3")?;
    file.write_all(&audio_bytes)?;

    // Create output stream
    let mut stream_handle = OutputStreamBuilder::open_default_stream()?;
    stream_handle.log_on_drop(false);
    // Load a sound from a file, using a path relative to Cargo.toml
    let file = BufReader::new(File::open("file.mp3").unwrap());
    // Note that the playback stops when the sink is dropped
    {
        let sink = rodio::play(&stream_handle.mixer(), file).unwrap();
        // wait for the sound to finish playing
        sink.sleep_until_end();
    }
    Ok(())
}

fn say_builtin(text: &str) -> AnyhowResult<()> {
    Command::new("say").arg("-v").arg("Moira").arg(text).spawn()?;
    Ok(())
}

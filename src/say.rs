use anyhow::Result as AnyhowResult;
use bytes::Bytes;
use rodio::OutputStreamBuilder;
use std::io::{BufReader, Cursor};
use std::process::Command;
use std::time::Duration;
use tracing::error;

pub fn say(text: &str, eleven_labs_key: Option<&str>) -> AnyhowResult<()> {
    if let Some(api_key) = eleven_labs_key {
        say_eleven_labs(text, api_key)
    } else {
        say_builtin(text)
    }
}

fn say_eleven_labs(text: &str, api_key: &str) -> AnyhowResult<()> {
    // Generate MP3 using ElevenLabs API
    let audio_bytes = match eleven_labs_request(text, api_key) {
        Ok(bytes) => bytes,
        Err(err) => {
            error!("ElevenLabs API request failed, falling back to built-in: {}", err);
            return say_builtin(text);
        }
    };

    // Create output stream
    let mut stream_handle = OutputStreamBuilder::open_default_stream()?;
    stream_handle.log_on_drop(false);

    // Use the audio bytes directly from memory via Cursor
    let cursor = Cursor::new(audio_bytes);
    let source = BufReader::new(cursor);

    // Play the audio
    {
        let sink = rodio::play(stream_handle.mixer(), source)?;
        // Wait for the sound to finish playing
        sink.sleep_until_end();
    }
    Ok(())
}

fn say_builtin(text: &str) -> AnyhowResult<()> {
    Command::new("say").arg("-v").arg("Moira").arg(text).spawn()?;
    Ok(())
}

// male britsh
const VOICE_ID: &str = "JBFqnCBsd6RMkjVDRZzb";

fn eleven_labs_request(text: &str, api_key: &str) -> AnyhowResult<Bytes> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{VOICE_ID}?output_format=mp3_44100_128");
    let response = client
        .post(&url)
        .header("xi-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "text": text,
            "model_id": "eleven_multilingual_v2"
        }))
        .send()?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Unexpected status code: {}", response.status()));
    }
    Ok(response.bytes()?)
}

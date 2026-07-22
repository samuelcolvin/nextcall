use anyhow::Result as AnyhowResult;
use bytes::Bytes;
use regex::Regex;
use rodio::OutputStreamBuilder;
use std::io::{BufReader, Cursor};
use std::process::Command;
use std::sync::LazyLock;
use std::thread;
use std::time::Duration;
use tracing::error;

use crate::camera;

/// How often playback checks the camera so an announcement is cut short the
/// moment the user joins the call — long titles must not talk over a live
/// meeting. Camera checks enumerate devices, so don't poll much faster.
const CAMERA_POLL: Duration = Duration::from_millis(500);

/// Rewrite rules making calendar-title shorthand pronounceable, applied in
/// order (`w/` must precede the bare-slash rule). Spoken text only — the
/// notification and tray keep the literal title.
static TTS_RULES: LazyLock<[(Regex, &'static str); 7]> = LazyLock::new(|| {
    [
        // strip quotes: the spoken message wraps the summary in its own
        (Regex::new(r#"["“”]"#).unwrap(), ""),
        // "1:1" / "1-1" / "2:1" -> "1 to 1" etc; the boundaries keep clock
        // times like "9:05" or "10:30" untouched
        (Regex::new(r"\b(\d)[:-](\d)\b").unwrap(), "$1 to $2"),
        // "121 Bob" -> "one to one Bob"
        (Regex::new(r"\b121\b").unwrap(), "one to one"),
        // "Alice <> Bob" / "Acme <-> Dave" -> "and"
        (Regex::new(r"\s*<-?>\s*").unwrap(), " and "),
        // "w/Eve" -> "with Eve"
        (Regex::new(r"(?i)\bw/\s*").unwrap(), "with "),
        // "Alice / Bob", "Alice // Bob" -> "and"; requires a letter on
        // both sides so dates ("7/29") and times stay untouched
        (Regex::new(r"(?i)([a-z)])\s*//?\s*([a-z(])").unwrap(), "$1 and $2"),
        // "Sync | Monthly" -> a comma pause
        (Regex::new(r"\s*\|\s*").unwrap(), ", "),
    ]
});

/// Rewrites an event summary so ElevenLabs (and `say`) pronounce common
/// calendar shorthand sensibly, e.g. "Alice/Bob 1:1" -> "Alice and Bob
/// 1 to 1". See [`TTS_RULES`] for the full list.
pub fn tts_friendly(summary: &str) -> String {
    let mut text = summary.to_string();
    for (regex, replacement) in TTS_RULES.iter() {
        if let std::borrow::Cow::Owned(replaced) = regex.replace_all(&text, *replacement) {
            text = replaced;
        }
    }
    text
}

/// Speaks `text`, via ElevenLabs when a key is configured, else the macOS
/// `say` command. Blocks until playback finishes — or is cut short because the
/// camera came on, i.e. the user joined the call mid-announcement.
pub fn say(text: &str, eleven_labs_key: Option<&str>) -> AnyhowResult<()> {
    if let Some(api_key) = eleven_labs_key {
        say_eleven_labs(text, api_key)
    } else {
        say_builtin(text)
    }
}

/// ElevenLabs TTS played through rodio; falls back to [`say_builtin`] if the
/// API request fails. Playback stops early if the camera becomes active.
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

    // Play the audio, watching the camera so joining the call cuts speech short.
    let sink = rodio::play(stream_handle.mixer(), source)?;
    while !sink.empty() {
        thread::sleep(CAMERA_POLL);
        if camera::camera_active() {
            sink.stop();
            break;
        }
    }
    Ok(())
}

/// Built-in fallback via the macOS `say` command; the process is killed if the
/// camera becomes active mid-utterance.
fn say_builtin(text: &str) -> AnyhowResult<()> {
    let mut child = Command::new("say").arg("-v").arg("Moira").arg(text).spawn()?;
    while child.try_wait()?.is_none() {
        thread::sleep(CAMERA_POLL);
        if camera::camera_active() {
            let _ = child.kill();
            // Reap the killed process so it doesn't linger as a zombie.
            let _ = child.wait();
            break;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::tts_friendly;

    /// One case per rewrite rule, using calendar-shaped titles.
    #[test]
    fn tts_friendly_rules() {
        // digit:digit, and slash between names
        assert_eq!(tts_friendly("Alice/Bob 1:1"), "Alice and Bob 1 to 1");
        assert_eq!(tts_friendly("2:1 - Charlie : Alice"), "2 to 1 - Charlie : Alice");
        assert_eq!(tts_friendly("Alice / Bob 1-1"), "Alice and Bob 1 to 1");
        // bare 121
        assert_eq!(tts_friendly("121 Bob M"), "one to one Bob M");
        // <> and <->, chained
        assert_eq!(tts_friendly("Alice <> Bob <> Charlie"), "Alice and Bob and Charlie");
        assert_eq!(tts_friendly("Acme <-> Dave"), "Acme and Dave");
        // w/ must win over the bare-slash rule
        assert_eq!(tts_friendly("Coffee w/Eve"), "Coffee with Eve");
        // double slash, and parens adjacent to a slash
        assert_eq!(tts_friendly("Alice // Bob"), "Alice and Bob");
        assert_eq!(tts_friendly("(F2F) / Acme"), "(F2F) and Acme");
        // pipe becomes a pause
        assert_eq!(tts_friendly("Zoom: Acme Sync | Monthly"), "Zoom: Acme Sync, Monthly");
        // straight and curly quotes are stripped (the message adds its own)
        assert_eq!(
            tts_friendly(r#"“Vibes” review with "the team""#),
            "Vibes review with the team"
        );
    }

    /// Clock times, dates and ampersands must pass through unchanged.
    #[test]
    fn tts_friendly_untouched() {
        assert_eq!(tts_friendly("9:05pm BA Flight"), "9:05pm BA Flight");
        assert_eq!(tts_friendly("10:30 am BST"), "10:30 am BST");
        assert_eq!(tts_friendly("Dryrun for 7/29 workshop"), "Dryrun for 7/29 workshop");
        assert_eq!(tts_friendly("Monthly Sales Q&A"), "Monthly Sales Q&A");
        assert_eq!(tts_friendly("Engineering Sync"), "Engineering Sync");
    }
}

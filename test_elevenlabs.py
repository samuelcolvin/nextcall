#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx2"]
# ///
"""Test the ElevenLabs key in `~/nextcall.toml`: speak "hello elevenlabs" and
always print the HTTP status and response. Plays the audio on success."""

import subprocess
import tempfile
import tomllib
from pathlib import Path

import httpx2

with open(Path.home() / "nextcall.toml", "rb") as f:
    key = tomllib.load(f)["eleven_labs_key"]

# Same voice, model and format as src/say.rs.
voice_id = "JBFqnCBsd6RMkjVDRZzb"
response = httpx2.post(
    f"https://api.elevenlabs.io/v1/text-to-speech/{voice_id}",
    params={"output_format": "mp3_44100_128"},
    headers={"xi-api-key": key},
    json={"text": "hello elevenlabs", "model_id": "eleven_multilingual_v2"},
)

print(f"status: {response.status_code}")
if response.status_code == 200:
    print(f"response: {len(response.content)} bytes of audio")
    with tempfile.NamedTemporaryFile(suffix=".mp3") as f:
        f.write(response.content)
        f.flush()
        subprocess.run(["afplay", f.name])
else:
    print(f"response: {response.text}")

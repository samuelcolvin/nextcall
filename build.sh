#!/bin/bash
set -e

cargo build --release

APP_NAME="Nextcall.app"
mkdir -p "$APP_NAME/Contents/MacOS"

# Replacing the executable of a running instance can crash it; stop it first.
killall nextcall 2>/dev/null || true

cp target/release/nextcall "$APP_NAME/Contents/MacOS/"

# Info.plist provides the CFBundleIdentifier that notifications require.
cp Info.plist "$APP_NAME/Contents/"

# Ad-hoc signature with a stable bundle ID so the notification permission
# grant survives rebuilds.
codesign --force --deep --sign - "$APP_NAME"

echo "Built $APP_NAME"

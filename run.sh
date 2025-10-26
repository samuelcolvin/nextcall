#!/bin/bash
set -e

cargo build --release

APP_NAME="Nextcall.app"
mkdir -p $APP_NAME/Contents/MacOS

cp target/release/nextcall $APP_NAME/Contents/MacOS/

# Copy Info.plist with bundle identifier
cp Info.plist $APP_NAME/Contents/

codesign -s - --force --deep $APP_NAME

echo "Running app..."
$APP_NAME/Contents/MacOS/nextcall

#!/bin/bash
set -e

cargo build --release

APP_NAME="Nextcall.app"
mkdir -p $APP_NAME/Contents/MacOS

cp target/release/nextcall $APP_NAME/Contents/MacOS/

codesign -s - --force --deep $APP_NAME

echo "Running app..."
Nextcall.app/Contents/MacOS/nextcall

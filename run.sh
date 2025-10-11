#!/bin/bash
set -e

echo "Building Rust binary..."
cargo build --release

echo "Creating app bundle structure..."
APP_NAME="Nextcall.app"
mkdir -p $APP_NAME/Contents/MacOS

echo "Copying binary to app bundle..."
cp target/release/nextcall $APP_NAME/Contents/MacOS/

echo "Signing app bundle..."
codesign -s - --force --deep $APP_NAME

echo "Running app..."
echo "Note: Output will appear in Console.app under 'nextcall' or run directly:"
echo "  $APP_NAME/Contents/MacOS/nextcall"
echo ""

# Run directly to see output
Nextcall.app/Contents/MacOS/nextcall

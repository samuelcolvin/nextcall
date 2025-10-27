#!/bin/bash
set -e

./build.sh

echo "Running app..."
$APP_NAME/Contents/MacOS/nextcall

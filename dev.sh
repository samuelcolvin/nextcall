#!/bin/bash
set -e

./build.sh

echo "Running app..."
nextcall_dev.app/Contents/MacOS/nextcall

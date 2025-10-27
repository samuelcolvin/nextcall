#!/bin/bash
set -e

./build.sh

echo "Running app..."
Nextcall.app/Contents/MacOS/nextcall

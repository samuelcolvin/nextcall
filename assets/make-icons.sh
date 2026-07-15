#!/bin/bash
set -e

# Regenerates the icon artifacts from the SVG sources using only tools that
# ship with macOS (sips + iconutil). The outputs (AppIcon.icns, tray-icon.png)
# are checked in, so this only needs re-running when the SVGs change.
cd "$(dirname "$0")"

# .icns wants each size at 1x and 2x; 2x of one size is 1x of the next.
ICONSET=AppIcon.iconset
rm -rf "$ICONSET"
mkdir "$ICONSET"
for size in 16 32 128 256 512; do
    sips -s format png -z "$size" "$size" appicon.svg --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    sips -s format png -z "$((size * 2))" "$((size * 2))" appicon.svg --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o AppIcon.icns
rm -rf "$ICONSET"

# 36px = 18pt @2x: the monochrome template image tray.m shows while idle.
sips -s format png -z 36 36 logo.svg --out tray-icon.png >/dev/null

echo "Generated AppIcon.icns and tray-icon.png"

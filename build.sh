#!/bin/bash
set -e

# Builds the app bundle. The default is the dev bundle (nextcall_dev.app) with
# its own bundle ID: two on-disk bundles sharing an ID confuse LaunchServices,
# e.g. a notification click can launch /Applications/Nextcall.app as a second
# instance. `./build.sh dist` builds the production Nextcall.app for `make install`.
if [ "$1" = "dist" ]; then
    APP_NAME="Nextcall.app"
    BUNDLE_ID="com.nextcall.app"
    BUNDLE_DISPLAY="Nextcall"
else
    APP_NAME="nextcall_dev.app"
    BUNDLE_ID="com.nextcall.app.dev"
    BUNDLE_DISPLAY="Nextcall Dev"
fi

cargo build --release

mkdir -p "$APP_NAME/Contents/MacOS"

# Replacing the executable of a running instance can crash it; stop any
# instance running from this bundle (other copies are left alone).
pkill -f "$PWD/$APP_NAME/Contents/MacOS/nextcall" 2>/dev/null || true

cp target/release/nextcall "$APP_NAME/Contents/MacOS/"

# App icon (Finder, Spotlight, notifications) and the tray's idle logo; both
# generated from assets/*.svg by assets/make-icons.sh and checked in.
mkdir -p "$APP_NAME/Contents/Resources"
cp assets/AppIcon.icns assets/tray-icon.png "$APP_NAME/Contents/Resources/"

# Info.plist provides the CFBundleIdentifier that notifications require;
# stamp this variant's bundle ID and display name into it.
sed -e "s|<string>com.nextcall.app</string>|<string>$BUNDLE_ID</string>|" \
    -e "s|<string>Nextcall</string>|<string>$BUNDLE_DISPLAY</string>|" \
    Info.plist > "$APP_NAME/Contents/Info.plist"

# Ad-hoc signature with a stable bundle ID so the notification permission
# grant survives rebuilds.
codesign --force --deep --sign - "$APP_NAME"

echo "Built $APP_NAME ($BUNDLE_ID)"

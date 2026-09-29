#!/bin/sh
# pinch-zoom uninstaller for macOS
#
#   curl -fsSL https://raw.githubusercontent.com/1000ldk/mouse-custom-own/main/uninstall.sh | sh
#
# Stops pinch-zoom, removes the login item, the app, its permissions
# and ~/Library/Application Support/pinch-zoom (including config.toml).
set -eu

bundle_id=com.github.1000ldk.pinch-zoom

pkill -x pinch-zoom 2>/dev/null || true
rm -f "$HOME/Library/LaunchAgents/$bundle_id.plist"
rm -rf "$HOME/Applications/pinch-zoom.app"
rm -rf "$HOME/Library/Application Support/pinch-zoom"
tccutil reset All "$bundle_id" >/dev/null 2>&1 || true

echo "pinch-zoom has been uninstalled."

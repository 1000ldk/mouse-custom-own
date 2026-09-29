#!/bin/sh
# pinch-zoom installer for macOS
#
#   curl -fsSL https://raw.githubusercontent.com/1000ldk/mouse-custom-own/main/install.sh | sh
#
# - Downloads the latest pinch-zoom.app from GitHub Releases
# - Installs it to ~/Applications (no admin rights needed)
# - Registers it to start at login, then starts it
# Re-running updates the app and keeps your config
# (~/Library/Application Support/pinch-zoom/config.toml).
set -eu

repo=1000ldk/mouse-custom-own
bundle_id=com.github.1000ldk.pinch-zoom
dest="$HOME/Applications"
app="$dest/pinch-zoom.app"
plist="$HOME/Library/LaunchAgents/$bundle_id.plist"
url="https://github.com/$repo/releases/latest/download/pinch-zoom-macos.zip"

if [ "$(uname -s)" != "Darwin" ]; then
    echo "This installer is for macOS. On Windows, use install.ps1 (see README)." >&2
    exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $url ..."
curl -fL --progress-bar -o "$tmp/pinch-zoom-macos.zip" "$url"
ditto -x -k "$tmp/pinch-zoom-macos.zip" "$tmp"

# Stop a running instance so the app can be replaced
if pgrep -x pinch-zoom >/dev/null 2>&1; then
    echo "Stopping running pinch-zoom..."
    pkill -x pinch-zoom || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        pgrep -x pinch-zoom >/dev/null 2>&1 || break
        sleep 0.5
    done
fi

updating=false
[ -d "$app" ] && updating=true
mkdir -p "$dest"
rm -rf "$app"
mv "$tmp/pinch-zoom.app" "$app"
xattr -dr com.apple.quarantine "$app" 2>/dev/null || true

if $updating; then
    # The app is not signed with an Apple certificate, so macOS treats every build as a new app
    # and the old permissions stop working. Remove them so macOS asks again cleanly.
    tccutil reset Accessibility "$bundle_id" >/dev/null 2>&1 || true
    tccutil reset ScreenCapture "$bundle_id" >/dev/null 2>&1 || true
fi

# Start at login (same as the menu item)
escaped=$(printf '%s' "$app" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g')
mkdir -p "$(dirname "$plist")"
cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>$bundle_id</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/open</string>
        <string>-a</string>
        <string>$escaped</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
EOF

open "$app"

echo ""
echo "Installed: $app"
echo "pinch-zoom is running in the menu bar and will start automatically at login."
echo "Allow \"Accessibility\" and \"Screen & System Audio Recording\" for pinch-zoom when macOS asks"
echo "(System Settings > Privacy & Security), then choose \"再起動\" (Restart) from its menu."
echo "Settings : $HOME/Library/Application Support/pinch-zoom/config.toml  (menu bar icon > 設定ファイルを開く)"

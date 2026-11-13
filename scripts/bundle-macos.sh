#!/usr/bin/env bash
# Builds target/release/tailgit.app with the app icon.
# Usage: scripts/bundle-macos.sh   (then: open target/release/tailgit.app)
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cargo build --release -p tailgit-app

APP=target/release/tailgit.app
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# Icon: export the masked 1024px icon, then build the .icns iconset.
./target/release/tailgit --export-icon "$WORK/icon_1024.png"
SET="$WORK/AppIcon.iconset"
mkdir "$SET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$WORK/icon_1024.png" --out "$SET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d "$WORK/icon_1024.png" --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$WORK/AppIcon.icns"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/tailgit "$APP/Contents/MacOS/tailgit"
cp "$WORK/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>TailGIT</string>
  <key>CFBundleDisplayName</key><string>TailGIT</string>
  <key>CFBundleIdentifier</key><string>dev.tailgit.app</string>
  <key>CFBundleExecutable</key><string>tailgit</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc sign so Gatekeeper accepts the local build.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true
touch "$APP"
echo "Built $APP"

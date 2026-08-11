#!/bin/zsh
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Build Clicker.app for macOS, Apple silicon only.
#
# The deliberate omissions, so nobody wonders:
#
#   * No Intel slice. The machines this is for are Apple silicon, and a
#     universal binary doubles the surface nobody tests.
#   * libmpv is NOT bundled. The app looks in Contents/Frameworks first and
#     then in /opt/homebrew/lib, so `brew install mpv` is the runtime
#     prerequisite. Bundling means carrying mpv's entire dylib closure —
#     FFmpeg and forty friends — with install_name_tool surgery on each; a
#     job worth doing when this ships to strangers, and pure liability while
#     the only user has Homebrew anyway.
#   * Ad-hoc signature, no notarization. Apple silicon requires *a*
#     signature, not an identity: a locally built app runs fine signed
#     ad-hoc. Distribution to other machines is where a Developer ID and
#     notarization come in, and this script stops honestly short of it.
#     One consequence worth knowing: macOS ties permission grants — the
#     local network prompt included — to the signature, and an ad-hoc
#     signature changes with every build, so a rebuild may ask again.
#
# Produces target/macos/Clicker.app and a zip beside it.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/target/macos"
APP="$OUT/Clicker.app"

if [[ "$(uname -m)" != "arm64" ]]; then
  echo "This build is Apple silicon only, and this machine is $(uname -m)." >&2
  exit 1
fi

echo "==> cargo build --release"
cargo build --release --manifest-path "$ROOT/Cargo.toml"

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$ROOT/target/release/clicker" "$APP/Contents/MacOS/Clicker"

# The version out of Cargo.toml, which build.ps1 keeps as the single source
# of truth on every platform.
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>Clicker</string>
	<key>CFBundleDisplayName</key><string>Clicker</string>
	<key>CFBundleIdentifier</key><string>io.github.mackid1993.Clicker</string>
	<key>CFBundleExecutable</key><string>Clicker</string>
	<key>CFBundleIconFile</key><string>Clicker.icns</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>${VERSION}</string>
	<key>CFBundleVersion</key><string>${VERSION}</string>
	<key>LSMinimumSystemVersion</key><string>12.0</string>
	<key>NSHighResolutionCapable</key><true/>
	<key>LSApplicationCategoryType</key><string>public.app-category.entertainment</string>
	<key>NSLocalNetworkUsageDescription</key>
	<string>Clicker connects to your Channels DVR server on your local network to play live TV and recordings.</string>
</dict>
</plist>
PLIST

# The icon, rendered from the same PNG the other platforms use. sips and
# iconutil ship with macOS, so this costs no dependency.
echo "==> icon"
ICONSET="$OUT/Clicker.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for size in 16 32 64 128 256 512; do
  sips -z $size $size "$ROOT/assets/clicker.png" \
    --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z $double $double "$ROOT/assets/clicker.png" \
    --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Clicker.icns"
rm -rf "$ICONSET"

# Apple silicon insists on a signature; it does not insist on an identity.
echo "==> ad-hoc signature"
codesign --force --deep -s - "$APP"

echo "==> zip"
ditto -c -k --keepParent "$APP" "$OUT/Clicker-macOS-${VERSION}-arm64.zip"

echo
echo "Built $APP"
echo "       $OUT/Clicker-macOS-${VERSION}-arm64.zip"
echo "Needs: brew install mpv"

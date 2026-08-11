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
#   * Signing rises to whatever is available. With a "Developer ID
#     Application" certificate in the keychain (or one named in
#     CLICKER_SIGN_IDENTITY) the app is signed properly — hardened
#     runtime, timestamp, the entitlements in packaging/macos — and if a
#     notarytool keychain profile called "notary" exists (or whatever
#     NOTARY_PROFILE names) it is
#     notarized and stapled too, which is what lets it run on machines
#     that are not this one. With none of that present it falls back to
#     the ad-hoc signature a local build needs and nothing more. One
#     ad-hoc consequence worth knowing: macOS ties permission grants —
#     the local network prompt included — to the signature, and an ad-hoc
#     signature changes with every build, so a rebuild may ask again.
#     A stable identity ends that.
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

# The licences, inside the bundle where they travel with it.
#
# Not a formality: Clicker's own terms are MIT and require the notice to go
# with the thing, the icon font compiled into the binary is MIT for the same
# reason, and the LGPL text is here because libmpv is what plays every frame.
# The Windows installer has carried these four files since the first release;
# the .app was shipping none of them.
echo "==> licences"
cp "$ROOT/LICENSE.md" "$ROOT/NOTICE.md" "$APP/Contents/Resources/"
mkdir -p "$APP/Contents/Resources/licenses"
cp "$ROOT"/licenses/* "$APP/Contents/Resources/licenses/"

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

# Sign with the best identity on hand. The grep is deliberately for
# "Developer ID Application": that is the kind Apple issues for apps
# distributed outside the App Store, and the only kind notarization accepts.
IDENTITY="${CLICKER_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null \
  | grep -m1 "Developer ID Application" | sed 's/.*"\(.*\)"/\1/' || true)}"

if [[ -n "$IDENTITY" ]]; then
  echo "==> signing as: $IDENTITY"
  codesign --force --options runtime --timestamp \
    --entitlements "$ROOT/packaging/macos/entitlements.plist" \
    -s "$IDENTITY" "$APP"
else
  # Apple silicon insists on a signature; it does not insist on an identity.
  echo "==> ad-hoc signature (no Developer ID certificate found)"
  codesign --force --deep -s - "$APP"
fi

ZIP="$OUT/Clicker-macOS-${VERSION}-arm64.zip"
echo "==> zip"
ditto -c -k --keepParent "$APP" "$ZIP"

# Notarization, when credentials have been stored once with:
#   xcrun notarytool store-credentials notary \
#     --apple-id <apple id email> --team-id <TEAMID> --password <app-specific>
#
# The profile name is deliberately generic and overridable: notarization
# credentials are per-developer, not per-app, so one profile serves every
# app on this machine and there is no reason for this project to demand a
# name of its own.
NOTARY_PROFILE="${NOTARY_PROFILE:-notary}"
if [[ -n "$IDENTITY" ]] \
  && xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" >/dev/null 2>&1; then
  echo "==> notarizing (this waits on Apple, usually a minute or two)"
  # Bounded, because "usually" is not "always": Apple's queue has slow
  # spells, and the first submission from a new team can sit for half an
  # hour. Without a limit a build machine waits forever on someone else's
  # service. The timeout only stops *waiting* — Apple carries on, and the
  # verdict can be collected later, which is what the message below says.
  if xcrun notarytool submit "$ZIP" \
       --keychain-profile "$NOTARY_PROFILE" --wait --timeout 30m; then
    xcrun stapler staple "$APP"
    # Re-zip so the archive carries the stapled ticket too. The first zip was
    # only ever the shipping box for Apple; this is the one people get.
    ditto -c -k --keepParent "$APP" "$ZIP"
  else
    echo
    echo "Notarization did not finish. The app is signed and usable here;" >&2
    echo "it is not yet stapled, so other Macs will warn about it." >&2
    echo "Check the verdict, then staple and re-zip by hand:" >&2
    echo "  xcrun notarytool history --keychain-profile $NOTARY_PROFILE" >&2
    echo "  xcrun stapler staple '$APP'" >&2
    echo "  ditto -c -k --keepParent '$APP' '$ZIP'" >&2
    exit 1
  fi
else
  echo "==> not notarized (no '$NOTARY_PROFILE' keychain profile)"
fi

echo
echo "Built $APP"
echo "       $ZIP"
echo "Needs: brew install mpv"

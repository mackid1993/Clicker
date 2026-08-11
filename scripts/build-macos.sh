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
#   * Universal when it can be, Apple silicon when it cannot. If the
#     x86_64 target is installed the two slices are built and `lipo`d into
#     one binary that runs on any Mac; if it is not, the build says so and
#     produces an arm64-only bundle rather than failing. Add the target
#     with: rustup target add x86_64-apple-darwin
#   * libmpv is bundled when third_party/mpv has been built, which is what
#     scripts/build-mpv.sh does: FFmpeg and mpv from their pinned tags,
#     LGPL only. Copying a package manager's copy instead is not an option —
#     Homebrew's FFmpeg is --enable-gpl — and an application somebody has to
#     run `brew install mpv` before using is not an application you can hand
#     to anyone. Without a staged build the bundle is still made, and still
#     falls back to a system libmpv, which is what a developer wants when
#     iterating.
#   * Signing rises to whatever is available, and nothing here is anybody's
#     in particular. With a "Developer ID Application" certificate in the
#     keychain — or one named in CLICKER_SIGN_IDENTITY — the app is signed
#     properly: hardened runtime, timestamp, the entitlements in
#     packaging/macos. With none, it falls back to the ad-hoc signature a
#     local build needs, so anyone can build this without an Apple account.
#
#     Notarizing is opt-in on top of that: it happens only when a notarytool
#     keychain profile exists, named by NOTARY_PROFILE and "notary" by
#     default. A machine that has never stored one does not attempt it.
#     --no-notarize skips it regardless.
#
#     No Apple ID, team identifier or password appears in this script or
#     anywhere else in the repository. The certificate comes from the
#     keychain and CI reads secrets belonging to whoever runs it.
#
#     One ad-hoc consequence worth knowing: macOS ties permission grants —
#     the local network prompt included — to the signature, and an ad-hoc
#     signature changes with every build, so a rebuild may ask again. A
#     stable identity ends that.
#
# Produces target/macos/Clicker.app and a zip beside it.

set -euo pipefail

# --no-notarize   sign, but do not wait on Apple. For iterating.
# --install       replace /Applications/Clicker.app with what was just built,
#                 so there is only ever one copy and it is this one.
NOTARIZE=yes
INSTALL=no
for argument in "$@"; do
  case "$argument" in
    --no-notarize) NOTARIZE=no ;;
    --install) INSTALL=yes ;;
    *) echo "unknown option: $argument" >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/target/macos"
APP="$OUT/Clicker.app"

if [[ "$(uname -m)" != "arm64" ]]; then
  echo "This build runs on Apple silicon, and this machine is $(uname -m)." >&2
  exit 1
fi

# Both architectures when the toolchain has both.
#
# Intel Macs are not the future and are still plenty of people's present, and
# a universal binary costs one extra compile rather than a second pipeline.
# The Intel slice is cross-compiled here on Apple silicon, which Rust and the
# system linker both do without ceremony.
ARM_BIN="$ROOT/target/aarch64-apple-darwin/release/clicker"
INTEL_BIN="$ROOT/target/x86_64-apple-darwin/release/clicker"

echo "==> cargo build --release (arm64)"
cargo build --release --target aarch64-apple-darwin --manifest-path "$ROOT/Cargo.toml"

UNIVERSAL=no
if rustc --print target-list >/dev/null 2>&1 &&    rustup target list --installed 2>/dev/null | grep -q '^x86_64-apple-darwin$'; then
  echo "==> cargo build --release (x86_64)"
  cargo build --release --target x86_64-apple-darwin --manifest-path "$ROOT/Cargo.toml"
  UNIVERSAL=yes
else
  echo "==> arm64 only (add the Intel slice with: rustup target add x86_64-apple-darwin)"
fi

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
if [[ "$UNIVERSAL" == "yes" ]]; then
  lipo -create "$ARM_BIN" "$INTEL_BIN" -output "$APP/Contents/MacOS/Clicker"
  echo "    universal: $(lipo -archs "$APP/Contents/MacOS/Clicker")"
else
  cp "$ARM_BIN" "$APP/Contents/MacOS/Clicker"
fi

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

# libmpv and FFmpeg, inside the bundle.
#
# Every library is copied in, its own id rewritten to @rpath, and every
# reference it makes to its build location rewritten the same way; the
# executable gets an rpath pointing at Frameworks. That is what makes the
# bundle self-contained rather than a set of paths that happened to exist on
# the build machine.
STAGE="$ROOT/third_party/mpv"
BUNDLED=no
if [[ -f "$STAGE/libmpv.2.dylib" ]]; then
  echo "==> bundling libmpv and FFmpeg"
  cp -a "$STAGE"/*.dylib "$APP/Contents/Frameworks/"

  # Everything they depend on that is not the system's, brought in with them.
  #
  # libmpv links libass and libplacebo, those link FreeType, HarfBuzz,
  # FriBidi and Fontconfig, and every one of those references the path it was
  # built at. A bundle that carries only the first layer works perfectly on
  # the machine that built it and on no other. So: walk the graph, copy
  # anything living under a package manager's prefix, and repeat until
  # nothing new appears.
  pending=1
  while [[ $pending -eq 1 ]]; do
    pending=0
    for lib in "$APP/Contents/Frameworks/"*.dylib; do
      [[ -e "$lib" ]] || continue
      while read -r ref; do
        case "$ref" in
          /opt/homebrew/*|/usr/local/*)
            refbase="$(basename "$ref")"
            if [[ ! -f "$APP/Contents/Frameworks/$refbase" ]]; then
              cp -L "$ref" "$APP/Contents/Frameworks/$refbase" 2>/dev/null && pending=1
              chmod u+w "$APP/Contents/Frameworks/$refbase" 2>/dev/null || true
            fi
            ;;
        esac
      done < <(otool -L "$lib" | awk 'NR>1 {print $1}')
    done
  done

  for lib in "$APP/Contents/Frameworks/"*.dylib; do
    base="$(basename "$lib")"
    install_name_tool -id "@rpath/$base" "$lib" 2>/dev/null || true
    # Anything it points at that we also carry becomes an @rpath reference.
    otool -L "$lib" | awk 'NR>1 {print $1}' | while read -r ref; do
      refbase="$(basename "$ref")"
      if [[ -f "$APP/Contents/Frameworks/$refbase" && "$ref" != @rpath/* ]]; then
        install_name_tool -change "$ref" "@rpath/$refbase" "$lib" 2>/dev/null || true
      fi
    done
  done

  install_name_tool -add_rpath "@executable_path/../Frameworks" \
    "$APP/Contents/MacOS/Clicker" 2>/dev/null || true
  BUNDLED=yes
else
  echo "==> no staged libmpv (run scripts/build-mpv.sh); the app will look for a system one"
fi

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
  # Inside out: a hardened bundle is only as signed as the code nested in it,
  # and codesign will not sign an outer bundle over unsigned libraries.
  for lib in "$APP/Contents/Frameworks/"*.dylib; do
    [[ -e "$lib" ]] || continue
    codesign --force --options runtime --timestamp -s "$IDENTITY" "$lib"
  done
  codesign --force --options runtime --timestamp \
    --entitlements "$ROOT/packaging/macos/entitlements.plist" \
    -s "$IDENTITY" "$APP"
else
  # Apple silicon insists on a signature; it does not insist on an identity.
  echo "==> ad-hoc signature (no Developer ID certificate found)"
  codesign --force --deep -s - "$APP"
fi

if [[ "$UNIVERSAL" == "yes" ]]; then
  ZIP="$OUT/Clicker-macOS-${VERSION}-universal.zip"
else
  ZIP="$OUT/Clicker-macOS-${VERSION}-arm64.zip"
fi
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
if [[ "$NOTARIZE" == "no" ]]; then
  echo "==> not notarizing (--no-notarize)"
elif [[ -n "$IDENTITY" ]] \
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

# Installing means there is one Clicker on this machine and it is this one.
# Without it a stale copy sits in /Applications looking exactly like the new
# build, and an afternoon goes into wondering why a change did not take.
if [[ "$INSTALL" == "yes" ]]; then
  echo "==> installing to /Applications"
  pkill -f "Clicker.app/Contents/MacOS/Clicker" 2>/dev/null || true
  rm -rf /Applications/Clicker.app
  ditto "$APP" /Applications/Clicker.app
fi

echo
echo "Built $APP"
echo "       $ZIP"
if [[ "$INSTALL" == "yes" ]]; then
  echo "Installed /Applications/Clicker.app"
fi
if [[ "$BUNDLED" == "yes" ]]; then
  echo "libmpv: bundled (LGPL, built by scripts/build-mpv.sh)"
else
  echo "libmpv: not bundled — needs one on the system (brew install mpv)"
fi

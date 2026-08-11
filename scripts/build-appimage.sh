#!/bin/bash
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Build Clicker as an AppImage: one file, made executable, double-clicked.
#
# Why this rather than the Flatpak, for now: a Flatpak does not use the
# system's graphics stack. It brings its own Mesa and its own libwayland and
# reaches the GPU through matched driver extensions, and when the match fails
# it falls back to software rendering without saying so — which is tearing,
# and a missing mouse pointer, on a machine where the same binary run
# natively is perfect. An AppImage uses the host's drivers, the host's
# compositor libraries and the host's cursor theme, because it is not a
# sandbox: it is the binary, in a squashfs, with a launcher.
#
# libmpv is deliberately NOT bundled, for the same reason it is not bundled
# in the .app on macOS: it is loaded by name at runtime, so the system's own
# copy is used, which is the copy that matches the system's FFmpeg and the
# system's video drivers. What that costs is one line of instruction —
#
#   Debian, Ubuntu:  sudo apt install libmpv2
#   Fedora:          sudo dnf install mpv-libs
#   Arch:            sudo pacman -S mpv
#
# — and what it buys is hardware decoding that works, which a bundled mpv
# built against the wrong drivers does not.
#
# Build it on the oldest distribution you intend to support: an AppImage's
# glibc requirement is whatever it was compiled against, and it cannot run
# on anything older.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARCH="$(uname -m)"
APP_ID="io.github.mackid1993.Clicker"
OUT="$ROOT/target/appimage"
APPDIR="$OUT/Clicker.AppDir"

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)

echo "==> cargo build --release ($ARCH)"
cargo build --release --manifest-path "$ROOT/Cargo.toml"

echo "==> AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
         "$APPDIR/usr/share/licenses/$APP_ID"

install -m755 "$ROOT/target/release/clicker" "$APPDIR/usr/bin/clicker"

# The licences travel with it, as they do in the installer and the .app.
install -m644 "$ROOT/LICENSE.md" "$ROOT/NOTICE.md" "$APPDIR/usr/share/licenses/$APP_ID/"
install -m644 "$ROOT"/licenses/* "$APPDIR/usr/share/licenses/$APP_ID/"

install -m644 "$ROOT/assets/clicker.png" \
  "$APPDIR/usr/share/icons/hicolor/512x512/apps/$APP_ID.png"
# AppImage looks for the icon at the top of the AppDir as well.
cp "$ROOT/assets/clicker.png" "$APPDIR/$APP_ID.png"

cat > "$APPDIR/usr/share/applications/$APP_ID.desktop" <<DESKTOP
[Desktop Entry]
Name=Clicker
Comment=An unofficial client for Channels DVR
Exec=clicker
Icon=$APP_ID
Terminal=false
Type=Application
Categories=AudioVideo;Video;TV;
DESKTOP
cp "$APPDIR/usr/share/applications/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"

# The launcher. Nothing clever: put our own bin first and run.
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/bash
HERE="$(dirname "$(readlink -f "${0}")")"
export PATH="${HERE}/usr/bin:${PATH}"
# Not LD_LIBRARY_PATH. Everything this needs — libmpv, Mesa, Wayland — comes
# from the system on purpose; pointing the loader at bundled copies first is
# exactly the mistake that makes the sandboxed build render in software.
exec "${HERE}/usr/bin/clicker" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

echo "==> appimagetool"
TOOL="$OUT/appimagetool-$ARCH.AppImage"
if [[ ! -x "$TOOL" ]]; then
  curl -sfL -o "$TOOL" \
    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
  chmod +x "$TOOL"
fi

# --appimage-extract-and-run because a CI container has no FUSE, and neither
# does every desktop.
echo "==> packaging"
ARCH="$ARCH" "$TOOL" --appimage-extract-and-run \
  "$APPDIR" "$OUT/Clicker-$VERSION-$ARCH.AppImage"

chmod +x "$OUT/Clicker-$VERSION-$ARCH.AppImage"
ls -lh "$OUT/Clicker-$VERSION-$ARCH.AppImage"
echo
echo "Built $OUT/Clicker-$VERSION-$ARCH.AppImage"
echo "Needs: libmpv on the system (apt install libmpv2)"

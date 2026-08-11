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
# libmpv IS bundled, from scripts/build-mpv.sh: FFmpeg and mpv from their
# pinned tags, LGPL only, exactly as the Windows installer carries them.
# Nothing is taken from the distribution, because a distribution's FFmpeg is
# frequently GPL and because an application that needs a package installed
# first is not one you can hand to anybody.
#
# What is NOT bundled is anything to do with graphics — Mesa, libwayland,
# libGL, the cursor theme. Those come from the machine, which is the entire
# difference between this and the Flatpak that tore and lost its pointer.
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

# libmpv and its FFmpeg, ours, beside the binary.
STAGE="$ROOT/third_party/mpv"
if [[ -f "$STAGE/libmpv.so.2" ]]; then
  echo "==> bundling libmpv and FFmpeg"
  mkdir -p "$APPDIR/usr/lib"
  cp -a "$STAGE"/*.so* "$APPDIR/usr/lib/"
else
  echo "no staged libmpv — run scripts/build-mpv.sh first" >&2
  exit 1
fi

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
# Deliberately no LD_LIBRARY_PATH.
#
# libmpv is found by the application itself, which looks in usr/lib beside
# its own binary — see mpv_candidates in src/platform/linux.rs — so the
# bundled player is used without putting anything in front of the system's
# loader. That matters: pointing LD_LIBRARY_PATH at bundled copies is how an
# application ends up using its own idea of Mesa, libwayland and libstdc++
# instead of the machine's, which is what made the sandboxed build render in
# software and lose its cursor.
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
echo "libmpv: bundled (LGPL, built by scripts/build-mpv.sh)"

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

  # What libass needs that a machine cannot be assumed to have.
  #
  # libunibreak is the one that proved this necessary. 22.04 ships
  # libunibreak.so.1 and 24.04 ships libunibreak.so.5, so libass — built here,
  # against the older soname — found nothing to load on a newer desktop, and
  # libmpv failed to open because of it. The application reported that as
  # libmpv being missing, which sent us looking for a file that was sitting
  # right there.
  #
  # Only the uncommon ones travel. freetype, fontconfig, harfbuzz, expat,
  # png and zlib are on every desktop with sonames that have not moved in a
  # decade, and fontconfig in particular reads the machine's own font
  # configuration — bundling that is how an application ends up with no fonts.
  for lib in "$APPDIR/usr/lib/"*.so*; do
    [[ -f "$lib" && ! -L "$lib" ]] || continue
    ldd "$lib" 2>/dev/null | awk '{print $1, $3}'
  done | sort -u | while read -r soname resolved; do
    case "$soname" in
      libunibreak.so.*|libfribidi.so.*|libgraphite2.so.*) ;;
      *) continue ;;
    esac
    [[ -f "${resolved:-}" ]] || continue
    base="$(basename "$resolved")"
    [[ -f "$APPDIR/usr/lib/$base" ]] && continue
    cp -L "$resolved" "$APPDIR/usr/lib/$base"
    chmod u+w "$APPDIR/usr/lib/$base"
    echo "    carrying $base"
  done

  # Teach each library to look beside itself.
  #
  # libmpv links libavcodec and the rest, and was built with an rpath naming
  # the directory it was compiled in — a path that exists on no other
  # machine. The loader does not search the folder a library happens to live
  # in, so dlopen fails on a dependency and the application reports the only
  # thing it can see: that libmpv could not be opened.
  #
  # $ORIGIN is the loader's word for "the directory this file is in", and it
  # is set on the bundled libraries alone. Doing the same job with
  # LD_LIBRARY_PATH would put this directory in front of the system's for
  # *every* library, which is how a bundle ends up using its own libstdc++ or
  # its own Mesa — the failure this AppImage exists to avoid.
  if command -v patchelf >/dev/null; then
    for lib in "$APPDIR/usr/lib/"*.so*; do
      [[ -f "$lib" && ! -L "$lib" ]] || continue
      patchelf --set-rpath '$ORIGIN' "$lib" 2>/dev/null || true
    done
  else
    echo "patchelf is missing: the bundled libraries will not find each other" >&2
    exit 1
  fi
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

# Tell the desktop what this window is, so it can find the icon.
#
# Wayland has no protocol for a window to carry its own icon. The compositor
# matches the surface's app_id — which the application sets to
# io.github.mackid1993.Clicker — against installed .desktop files, and uses
# the icon named there. No desktop file, no icon: GNOME shows the grey
# fallback in the dock and in Alt-Tab however good the artwork in the bundle
# is. An AppImage installs nothing by itself, so this puts the two files
# where the desktop looks, once, pointing Exec at wherever this AppImage
# happens to live.
#
# Written only when absent, so moving or deleting the AppImage leaves at most
# a stale menu entry, and never on a system where AppImageLauncher or
# appimaged has already done the same job properly.
APP_ID=io.github.mackid1993.Clicker
DESKTOP="${XDG_DATA_HOME:-$HOME/.local/share}/applications/${APP_ID}.desktop"
ICON="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/512x512/apps/${APP_ID}.png"
if [ ! -e "$DESKTOP" ] && [ -w "$(dirname "$(dirname "$DESKTOP")")" ] 2>/dev/null; then
  mkdir -p "$(dirname "$DESKTOP")" "$(dirname "$ICON")" 2>/dev/null
  cp "${HERE}/${APP_ID}.png" "$ICON" 2>/dev/null
  sed "s|^Exec=.*|Exec=$(readlink -f "${APPIMAGE:-$0}")|" \
    "${HERE}/${APP_ID}.desktop" > "$DESKTOP" 2>/dev/null
  command -v update-desktop-database >/dev/null \
    && update-desktop-database "$(dirname "$DESKTOP")" 2>/dev/null
fi
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

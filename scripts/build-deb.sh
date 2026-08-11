#!/bin/bash
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Build a .deb: double-clicked, installed, in the menu with its icon.
#
# This replaced the AppImage, which replaced the Flatpak, and the reasons are
# worth keeping.
#
# The Flatpak went because it does not use the machine's graphics stack. It
# brings its own Mesa and its own libwayland and reaches the GPU through
# matched driver extensions, and when the match fails it renders in software
# without saying so — tearing, and a missing mouse pointer, on hardware where
# the same binary run natively was perfect.
#
# The AppImage went because of how it meets a person. A browser download has
# no execute bit, and even after `chmod +x` the GNOME file manager refuses to
# launch a binary on a double click — "Run as a Program" from a context menu
# is the only door. It has no menu entry and no icon until something
# integrates it. Every one of those is the format, not the application, and
# nothing shipped inside the file can change any of them.
#
# A .deb is installed. Binary and libraries under /usr/lib/clicker, desktop
# entry where the desktop looks, icon where the theme looks. It appears in the
# menu, it launches on a click, and `apt remove clicker` takes it away again.
#
# libmpv and FFmpeg are bundled, from scripts/build-mpv.sh: their pinned tags,
# LGPL only, exactly as the Windows installer and the .app carry them. What is
# NOT bundled is graphics — Mesa, libGL, libwayland, libva, the cursor theme —
# because hardware decoding and the compositor have to be the machine's own.
# That distinction is the whole lesson of the Flatpak.
#
#   ./scripts/build-deb.sh          build target/deb/clicker_<version>_<arch>.deb
#
# Build it on the oldest release you mean to support: a .deb cannot run on a
# glibc older than the one it was compiled against.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_ID="io.github.mackid1993.Clicker"
OUT="$ROOT/target/deb"
PKGDIR="$OUT/clicker-pkg"

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
ARCH=$(dpkg --print-architecture)

for tool in dpkg-deb dpkg-shlibdeps cargo; do
  command -v "$tool" >/dev/null || {
    echo "missing build tool: $tool (apt install dpkg-dev)" >&2
    exit 1
  }
done

# Keep the machine's directory layout out of the binary, as build.ps1 does on
# Windows. rustc records the path of every source file it compiles, including
# the crate registry under $HOME, and those strings survive into the shipped
# executable. `strip = true` removes symbols; this removes the paths baked
# into panic messages, which stripping does not touch.
export RUSTFLAGS="--remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$ROOT=/clicker --remap-path-prefix=$HOME=/home ${RUSTFLAGS:-}"

echo "==> cargo build --release ($ARCH)"
cargo build --release --manifest-path "$ROOT/Cargo.toml"

echo "==> laying out the package"
rm -rf "$PKGDIR"
mkdir -p "$PKGDIR/DEBIAN" \
         "$PKGDIR/usr/bin" \
         "$PKGDIR/usr/lib/clicker" \
         "$PKGDIR/usr/share/applications" \
         "$PKGDIR/usr/share/icons/hicolor/512x512/apps" \
         "$PKGDIR/usr/share/doc/clicker"

# The binary lives beside its libraries rather than in /usr/bin, and /usr/bin
# holds a symlink to it.
#
# That is not tidiness. The application finds libmpv by looking next to its own
# executable (see mpv_candidates in src/platform/linux.rs), and on Linux
# `current_exe` reads /proc/self/exe, which resolves the symlink — so a person
# typing `clicker` and the desktop entry launching it both arrive at
# /usr/lib/clicker/clicker with the player sitting right there.
install -m755 "$ROOT/target/release/clicker" "$PKGDIR/usr/lib/clicker/clicker"
ln -sf ../lib/clicker/clicker "$PKGDIR/usr/bin/clicker"

STAGE="$ROOT/third_party/mpv"
if [[ ! -f "$STAGE/libmpv.so.2" ]]; then
  echo "no staged libmpv — run scripts/build-mpv.sh first" >&2
  exit 1
fi

echo "==> bundling libmpv and FFmpeg"
cp -a "$STAGE"/*.so* "$PKGDIR/usr/lib/clicker/"

# What libass needs that a machine cannot be assumed to have.
#
# libunibreak is why this exists: its soname is libunibreak.so.1 on 22.04 and
# libunibreak.so.5 on 24.04, so a libass built against the older one found
# nothing to load on a newer desktop and took libmpv down with it. The
# uncommon text libraries travel with the player.
#
# The common ones deliberately do not. freetype, fontconfig, harfbuzz and
# zlib are on every desktop with sonames that have not moved in years, and a
# bundled fontconfig reads the machine's font configuration through its own
# idea of where that lives — which is how an application ends up with no
# fonts at all.
for lib in "$PKGDIR/usr/lib/clicker/"*.so*; do
  [[ -f "$lib" && ! -L "$lib" ]] || continue
  ldd "$lib" 2>/dev/null | awk '{print $1, $3}'
done | sort -u | while read -r soname resolved; do
  case "$soname" in
    libunibreak.so.*|libfribidi.so.*|libgraphite2.so.*) ;;
    *) continue ;;
  esac
  [[ -f "${resolved:-}" ]] || continue
  base="$(basename "$resolved")"
  [[ -f "$PKGDIR/usr/lib/clicker/$base" ]] && continue
  cp -L "$resolved" "$PKGDIR/usr/lib/clicker/$base"
  chmod u+w "$PKGDIR/usr/lib/clicker/$base"
  echo "    carrying $base"
done

# Teach every bundled library to look beside itself.
#
# libmpv links libavcodec and the rest and was built with an rpath naming the
# directory it was compiled in, which exists on no other machine. The loader
# does not search the folder a library happens to sit in, so dlopen fails on a
# dependency and the application can only report that libmpv would not open.
#
# $ORIGIN is the loader's word for "the directory this file is in", and it is
# set on the bundled libraries alone. Doing this with LD_LIBRARY_PATH instead
# would put our directory in front of the system's for every library in the
# process, which is how a bundle ends up using its own libstdc++ or its own
# Mesa.
if command -v patchelf >/dev/null; then
  for lib in "$PKGDIR/usr/lib/clicker/"*.so*; do
    [[ -f "$lib" && ! -L "$lib" ]] || continue
    patchelf --set-rpath '$ORIGIN' "$lib" 2>/dev/null || true
  done
  patchelf --set-rpath '$ORIGIN' "$PKGDIR/usr/lib/clicker/clicker" 2>/dev/null || true
else
  echo "patchelf is missing: the bundled libraries will not find each other" >&2
  exit 1
fi

install -m644 "$ROOT/assets/clicker.png" \
  "$PKGDIR/usr/share/icons/hicolor/512x512/apps/$APP_ID.png"

# Icon= and the application's app_id are the same string on purpose. Wayland
# has no protocol for a window to carry its own icon: the compositor matches
# the surface's app_id against installed desktop entries and uses the icon
# named in the one it finds. A mismatch here is a grey square in the dock.
cat > "$PKGDIR/usr/share/applications/$APP_ID.desktop" <<DESKTOP
[Desktop Entry]
Name=Clicker
Comment=An unofficial client for Channels DVR
Exec=clicker
Icon=$APP_ID
Terminal=false
Type=Application
Categories=AudioVideo;Video;TV;
StartupWMClass=$APP_ID
DESKTOP

# Debian wants the licence at a fixed path, and the rest travel beside it as
# they do in the installer and the .app.
cp "$ROOT/LICENSE.md" "$PKGDIR/usr/share/doc/clicker/copyright"
cp "$ROOT/NOTICE.md" "$PKGDIR/usr/share/doc/clicker/"
mkdir -p "$PKGDIR/usr/share/doc/clicker/licenses"
cp "$ROOT"/licenses/* "$PKGDIR/usr/share/doc/clicker/licenses/"

# ------------------------------------------------------------- dependencies ---
#
# Worked out from the binaries, not written down here.
#
# dpkg-shlibdeps reads every ELF file, resolves each library it needs to the
# package that ships it, and emits the versioned dependency line Debian
# expects. Hand-listing them goes stale the moment a crate gains a dependency,
# and gets it wrong in the direction that matters: a missing entry is an
# install that succeeds and an application that will not start.
#
# -l points it at our own bundled libraries so they are recognised as ours and
# not looked for in any package. --ignore-missing-info because the private
# libraries have no shlibs file, which is the entire point of them.
echo "==> dependencies"
DEPWORK="$OUT/shlibdeps"
rm -rf "$DEPWORK"
mkdir -p "$DEPWORK/debian"
cat > "$DEPWORK/debian/control" <<CONTROL
Source: clicker
Package: clicker
Architecture: $ARCH
CONTROL

# Every ELF file in the package, not only the executable.
#
# Reading the binary alone produced a package that declared libc, libgcc,
# gdk-pixbuf, glib and GTK — everything the application links directly — and
# nothing at all for the player. libmpv is dlopened rather than linked, so it
# contributes no dependency of its own, and *its* needs are the long list:
# freetype, fontconfig, harfbuzz, expat, lcms2, libva, libvdpau, libdrm, X11.
# Those happen to be on every desktop, which is exactly what makes the omission
# dangerous — it installs cleanly and then fails to play anything on the one
# machine that is missing one.
ELVES=("$PKGDIR/usr/lib/clicker/clicker")
for lib in "$PKGDIR/usr/lib/clicker/"*.so*; do
  [[ -f "$lib" && ! -L "$lib" ]] && ELVES+=("$lib")
done
echo "    reading ${#ELVES[@]} binaries"

(
  cd "$DEPWORK"
  dpkg-shlibdeps -O --ignore-missing-info \
    -l"$PKGDIR/usr/lib/clicker" \
    "${ELVES[@]}" 2>/dev/null
) > "$OUT/deps.txt" || true

DEPENDS=$(sed -n 's/^shlibs:Depends=//p' "$OUT/deps.txt" | head -1)

# Second way of asking, for when the first will not answer.
#
# dpkg-shlibdeps wants to be run from inside a source package and is particular
# about it. When it declines, the same question gets asked directly: every
# library the binary resolves to, and the package that owns each file. No
# version constraints, which is a real loss — but a package that names its
# dependencies unversioned installs and runs, and one that names none at all
# installs and then does nothing.
if [[ -z "$DEPENDS" ]]; then
  echo "    dpkg-shlibdeps declined; resolving with ldd and dpkg -S"
  DEPENDS=$(for elf in "${ELVES[@]}"; do ldd "$elf" 2>/dev/null; done \
    | awk '{print $3}' | grep '^/' | sort -u \
    | while read -r path; do dpkg -S "$path" 2>/dev/null | cut -d: -f1; done \
    | sort -u | paste -sd, - | sed 's/,/, /g')
fi

if [[ -z "$DEPENDS" ]]; then
  echo "could not work out any dependencies; refusing to ship a package that" >&2
  echo "declares none and then fails to start on a clean machine" >&2
  exit 1
fi
echo "    $DEPENDS"

cat > "$PKGDIR/DEBIAN/control" <<CONTROL
Package: clicker
Version: $VERSION
Section: video
Priority: optional
Architecture: $ARCH
Depends: $DEPENDS
Maintainer: David Brustein <mackid1993@users.noreply.github.com>
Homepage: https://github.com/mackid1993/Clicker
Description: Unofficial native client for Channels DVR
 A native client for a Channels DVR server: live TV with a real guide,
 recordings, and downloads for watching away from the network.
 .
 Playback is mpv, built LGPL and bundled, so nothing needs installing first.
 Graphics, hardware decoding and the cursor theme come from the machine.
CONTROL

# The icon cache and the desktop database are the difference between
# "installed" and "in the menu". Both are cheap and both are skipped silently
# on a system that has neither.
cat > "$PKGDIR/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if [ "$1" = "configure" ]; then
  command -v update-desktop-database >/dev/null 2>&1 \
    && update-desktop-database -q /usr/share/applications || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 \
    && gtk-update-icon-cache -q -f -t /usr/share/icons/hicolor || true
fi
POSTINST

cat > "$PKGDIR/DEBIAN/postrm" <<'POSTRM'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
  command -v update-desktop-database >/dev/null 2>&1 \
    && update-desktop-database -q /usr/share/applications || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 \
    && gtk-update-icon-cache -q -f -t /usr/share/icons/hicolor || true
fi
POSTRM

chmod 755 "$PKGDIR/DEBIAN/postinst" "$PKGDIR/DEBIAN/postrm"

echo "==> packaging"
DEB="$OUT/clicker_${VERSION}_${ARCH}.deb"
dpkg-deb --root-owner-group --build "$PKGDIR" "$DEB" >/dev/null

# Read back what was built rather than trusting what was written.
echo
dpkg-deb --info "$DEB" | sed 's/^/    /'
echo
ls -lh "$DEB"
echo
echo "Built $DEB"
echo "libmpv: bundled (LGPL, built by scripts/build-mpv.sh)"
echo "Install with: sudo apt install $DEB"

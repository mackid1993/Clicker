#!/bin/bash
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Build libmpv and FFmpeg from source, LGPL only, for macOS and Linux.
#
# The same job `scripts/build-mpv.ps1` does for Windows, and for the same
# reason: what a distribution ships must not be GPL. mpv itself is LGPL when
# built with `-Dgpl=false`, and FFmpeg is LGPL when configured
# `--disable-gpl`, and neither is true of the copies a package manager
# provides. Homebrew's FFmpeg is `--enable-gpl` and its mpv links
# librubberband; a build that copies those into an application ships a
# GPL-combined work inside an MIT one, which is precisely what `build.ps1`
# refuses to do on Windows by reading the licence string back out of the
# binary before it packages anything.
#
# The result is staged into third_party/mpv, where build-macos.sh and
# build-appimage.sh look for it. Nothing is installed on the machine and
# nothing outside the repository is written.
#
#   ./scripts/build-mpv.sh              build what is missing
#   ./scripts/build-mpv.sh --clean      start again from nothing
#
# Build-time dependencies, which are not shipped: meson, ninja, nasm,
# pkg-config, and libass and libplacebo (both permissive or LGPL, and both
# bundled alongside).
#
#   macOS:  brew install meson ninja nasm pkg-config libass libplacebo
#   Linux:  apt install meson ninja-build nasm pkg-config \
#                       libass-dev libplacebo-dev

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
THIRD="$ROOT/third_party"
FFMPEG_SRC="$THIRD/ffmpeg-src"
MPV_SRC="$THIRD/mpv-src"
DEPS="$THIRD/mpv-deps"
STAGE="$THIRD/mpv"

# The same tags Windows is pinned to. Moving either is a deliberate act:
# read mpv's release notes first, because the render API is the part of it
# this application depends on.
FFMPEG_TAG="n7.1.1"
MPV_TAG="v0.41.0"

JOBS="$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)"
case "$(uname -s)" in
  Darwin) LIBEXT="dylib" ;;
  *)      LIBEXT="so" ;;
esac

if [[ "${1:-}" == "--clean" ]]; then
  rm -rf "$FFMPEG_SRC" "$MPV_SRC" "$DEPS" "$STAGE"
fi

for tool in meson ninja nasm pkg-config git make; do
  command -v "$tool" >/dev/null || {
    echo "missing build tool: $tool — see the header of this script" >&2
    exit 1
  }
done

mkdir -p "$THIRD"

# ------------------------------------------------------- libass, libplacebo ---
#
# Windows takes these from MSYS2, which has versions new enough. An Ubuntu
# 22.04 runner does not — mpv 0.41 wants libass 0.17 and libplacebo 7.349,
# and 22.04 ships neither — so they are built from source there, from the
# same tags the Flatpak manifest pins. Both are permissive or LGPL, so they
# are bundled alongside without argument.
LIBASS_TAG="0.17.4"
LIBPLACEBO_TAG="v7.349.0"

have_new_enough() {
  pkg-config --exists --print-errors "$1 >= $2" 2>/dev/null
}

if ! have_new_enough libass 0.17.0; then
  if [[ ! -f "$DEPS/lib/libass.$LIBEXT" ]]; then
    echo "==> building libass $LIBASS_TAG"
    [[ -d "$THIRD/libass-src" ]] || git clone --depth 1 --branch "$LIBASS_TAG" \
      https://github.com/libass/libass.git "$THIRD/libass-src"
    (
      cd "$THIRD/libass-src"
      ./autogen.sh
      ./configure --prefix="$DEPS" --disable-static
      make -j"$JOBS" && make install
    )
  fi
fi

if ! have_new_enough libplacebo 7.349; then
  if [[ ! -f "$DEPS/lib/libplacebo.$LIBEXT" ]]; then
    echo "==> building libplacebo $LIBPLACEBO_TAG"
    [[ -d "$THIRD/libplacebo-src" ]] || git clone --depth 1 --branch "$LIBPLACEBO_TAG" \
      --recursive https://code.videolan.org/videolan/libplacebo.git "$THIRD/libplacebo-src"
    (
      cd "$THIRD/libplacebo-src"
      # OpenGL only: this application drives mpv through the OpenGL render
      # API and never touches the Vulkan half.
      meson setup build --prefix="$DEPS" --libdir=lib \
        -Dvulkan=disabled -Dshaderc=disabled -Dglslang=disabled \
        -Ddemos=false -Dtests=false
      ninja -C build && ninja -C build install
    )
  fi
fi

export PKG_CONFIG_PATH="$DEPS/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

# ------------------------------------------------------------------ FFmpeg ---

if [[ ! -f "$FFMPEG_SRC/configure" ]]; then
  echo "==> cloning FFmpeg $FFMPEG_TAG"
  git clone --depth 1 --branch "$FFMPEG_TAG" https://github.com/FFmpeg/FFmpeg.git "$FFMPEG_SRC"
fi

if [[ ! -f "$DEPS/lib/libavcodec.$LIBEXT" ]]; then
  echo "==> configuring FFmpeg (LGPL, decode only)"
  (
    cd "$FFMPEG_SRC"
    # --disable-autodetect so nothing on the build machine is picked up and
    # silently becomes either a dependency or a licence problem. The two
    # hardware paths below are each platform's own, and neither is GPL.
    EXTRA=""
    if [[ "$(uname -s)" == "Darwin" ]]; then
      EXTRA="--enable-videotoolbox --enable-audiotoolbox"
    else
      EXTRA="--enable-vaapi --enable-vdpau"
    fi
    ./configure \
      --prefix="$DEPS" \
      --enable-shared \
      --disable-static \
      --disable-gpl \
      --disable-nonfree \
      --disable-autodetect \
      $EXTRA \
      --disable-programs \
      --disable-doc \
      --disable-avdevice \
      --disable-debug
    make -j"$JOBS"
    make install
  )
fi

# --------------------------------------------------------------------- mpv ---

if [[ ! -d "$MPV_SRC/.git" ]]; then
  echo "==> cloning mpv $MPV_TAG"
  git clone --depth 1 --branch "$MPV_TAG" https://github.com/mpv-player/mpv.git "$MPV_SRC"
fi

if [[ ! -f "$DEPS/lib/libmpv.$LIBEXT" ]]; then
  echo "==> configuring mpv (LGPL, library only)"
  (
    cd "$MPV_SRC"
    # Our FFmpeg first on the pkg-config path, the system's after it, so
    # libass and libplacebo are found while FFmpeg is unmistakably ours.
    export PKG_CONFIG_PATH="$DEPS/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
    rm -rf build
    # -Dgpl=false is the whole point: it is what makes libmpv LGPL-2.1.
    # -Dlua and -Djavascript disabled because the scripts they run are turned
    # off anyway (see NO_SCRIPTS in src/mpv.rs) and LuaJIT's generated code is
    # what macOS's hardened runtime kills.
    meson setup build \
      --prefix="$DEPS" \
      --libdir=lib \
      -Dgpl=false \
      -Dlibmpv=true \
      -Dcplayer=false \
      -Dlua=disabled \
      -Djavascript=disabled \
      -Dmanpage-build=disabled \
      -Dtests=false
    ninja -C build
    ninja -C build install
  )
fi

# ------------------------------------------------------------------- stage ---

echo "==> staging"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -a "$DEPS/lib/"libmpv.* "$STAGE/" 2>/dev/null || true
cp -a "$DEPS/lib/"libav*.* "$DEPS/lib/"libsw*.* "$STAGE/" 2>/dev/null || true
# libass and libplacebo too, when they were built here rather than found.
cp -a "$DEPS/lib/"libass.* "$DEPS/lib/"libplacebo.* "$STAGE/" 2>/dev/null || true
# Neither the .a files nor the unversioned development symlinks belong in a
# staged runtime.
rm -f "$STAGE"/*.a "$STAGE"/*.la 2>/dev/null || true

# ----------------------------------------------------------------- licence ---
#
# Asked of the binary rather than assumed from the flags above, which is what
# makes this a check rather than a comment. build.ps1 does the same before it
# packages anything on Windows.
echo "==> licence"
LICENSE_STRING=$(strings "$STAGE"/libmpv.* 2>/dev/null | grep -m1 -E "GPL version|LGPL version" || true)
echo "    libmpv says: ${LICENSE_STRING:-<nothing found>}"
if echo "$LICENSE_STRING" | grep -q "^GPL"; then
  echo "libmpv was built GPL; refusing to stage it" >&2
  exit 1
fi
if strings "$DEPS/lib/"libavcodec.* 2>/dev/null | grep -q -- "--enable-gpl"; then
  echo "FFmpeg was configured --enable-gpl; refusing to stage it" >&2
  exit 1
fi

ls -1 "$STAGE"
echo
echo "Staged LGPL libmpv and FFmpeg into $STAGE"

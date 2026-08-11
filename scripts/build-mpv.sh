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
# build-deb.sh look for it. Nothing is installed on the machine and
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
# and 22.04 ships neither — so they are built from source there, from the tags
# below. Both are permissive or LGPL, so they are bundled alongside without
# argument.
LIBASS_TAG="0.17.4"
LIBPLACEBO_TAG="v7.349.0"

have_new_enough() {
  pkg-config --exists --print-errors "$1 >= $2" 2>/dev/null
}

# Nothing in this repository's root may be called install.sh, install-sh or
# shtool.
#
# Autoconf picks a package's auxiliary directory by searching `.`, `..` and
# `../..` for a file with one of those names. libass unpacks to
# third_party/libass-src, so `../..` is the root of this repository — and when
# a curl installer lived there as install.sh, libtoolize decided the
# repository root was libass's aux directory, wrote ltmain.sh into it, and
# automake then stopped on `required file './ltmain.sh' not found`. It looked
# for all the world like a missing libtool.
#
# The installer is setup.sh for that reason and no other.
#
# A source tree from an attempt that died halfway is not a source tree.
#
# The first run of this on a machine without autoconf stopped inside
# libass's autogen.sh, leaving a directory that looked built enough to skip
# cloning and was missing the files automake needs — so the retry failed
# differently, with `required file './ltmain.sh' not found`, and looked like a
# missing package rather than a poisoned checkout. Anything already here is
# put back the way it was cloned before it is used again.
pristine() {
  local dir="$1"
  [[ -d "$dir/.git" ]] || return 0
  git -C "$dir" reset --hard --quiet
  git -C "$dir" clean -xfdq
}

if ! have_new_enough libass 0.17.0; then
  if [[ ! -f "$DEPS/lib/libass.$LIBEXT" ]]; then
    echo "==> building libass $LIBASS_TAG"
    [[ -d "$THIRD/libass-src" ]] || git clone --depth 1 --branch "$LIBASS_TAG" \
      https://github.com/libass/libass.git "$THIRD/libass-src"
    pristine "$THIRD/libass-src"
    (
      cd "$THIRD/libass-src"
      # libtoolize before autogen.sh rather than relying on autoreconf to
      # infer it. It usually does; when it does not, automake stops on a
      # missing ltmain.sh, which reads as a broken machine rather than a
      # step that was skipped.
      libtoolize --copy --force >/dev/null 2>&1 || true
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
    # Audio outputs, named rather than left to detection.
    #
    # This is the bug that shipped: every audio output in mpv is `auto` by
    # default, meaning "build it if the headers happen to be installed", and
    # the runners had none of them. The build succeeded, the licence gate
    # passed, the package installed, video played — and the only audio output
    # compiled in was `null`. Silence, with nothing anywhere saying why.
    #
    # `enabled` rather than `auto` for the two that matter, so a machine
    # missing the headers fails the build instead of producing a mute player.
    # PipeWire stays `auto` because 22.04's is too old to insist on, and a
    # PipeWire desktop is served by the PulseAudio output through its own
    # compatibility layer regardless.
    AUDIO="-Dalsa=enabled -Dpulse=enabled -Dpipewire=auto -Djack=disabled -Dsndio=disabled"
    if [[ "$(uname -s)" == "Darwin" ]]; then
      AUDIO="-Dcoreaudio=enabled"
    fi
    # shellcheck disable=SC2086
    meson setup build \
      --prefix="$DEPS" \
      --libdir=lib \
      -Dgpl=false \
      -Dlibmpv=true \
      -Dcplayer=false \
      -Dlua=disabled \
      -Djavascript=disabled \
      -Dmanpage-build=disabled \
      -Dtests=false \
      $AUDIO
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
# Read out of the libraries that were staged, not out of the source tree and
# not assumed from the flags above.
#
# Two earlier versions of this check were wrong in opposite directions. The
# first grepped for a licence string it never matched and printed "<nothing
# found>" — then passed, having verified nothing. The second read FFmpeg's
# config.h from ffbuild/, which is where the log lives; configure writes the
# header to the build root, so a perfectly good LGPL build was refused as
# unverifiable.
#
# The staged binary settles both. configure bakes its answer into
# avutil_license() as a string literal, so it is in the object file no matter
# what the build directory looks like afterwards, and it cannot disagree with
# the library it is compiled into.
echo "==> licence"

# The alternation is ordered and anchored on purpose: "LGPL version 2.1 or
# later" contains "GPL version 2.1 or later", so a pattern that matched the
# shorter one first would read every LGPL build as GPL.
licence_of() {
  grep -a -oE 'LGPL version [0-9.]+ or later|GPL version [0-9.]+ or later|nonfree and unredistributable' \
    "$1" 2>/dev/null | sort -u | head -1
}

FOUND=""
for lib in "$STAGE"/libavutil*; do
  [[ -f "$lib" && ! -L "$lib" ]] || continue
  FOUND="$(licence_of "$lib")"
  if [[ -n "$FOUND" ]]; then break; fi
done

if [[ -n "$FOUND" ]]; then
  case "$FOUND" in
    LGPL*) echo "    FFmpeg: $FOUND" ;;
    *)     echo "FFmpeg is $FOUND, not LGPL; refusing to stage it" >&2; exit 1 ;;
  esac
elif [[ -f "$FFMPEG_SRC/config.h" ]]; then
  # configure writes its header to the build root, which for the in-tree
  # build above is the source root. Second rather than first because a header
  # left over from an earlier configure would describe a build that is no
  # longer there, while the string is inside the library being staged.
  if grep -qE "^#define CONFIG_(GPL|NONFREE) 1" "$FFMPEG_SRC/config.h"; then
    echo "FFmpeg was configured with GPL or nonfree components; refusing to stage it" >&2
    exit 1
  fi
  echo "    FFmpeg: CONFIG_GPL 0, CONFIG_NONFREE 0 (from config.h)"
else
  echo "no licence string in the staged libavutil and no config.h beside it;" >&2
  echo "refusing to stage an unverified build" >&2
  exit 1
fi

# mpv's own licence is a compile-time constant rather than a string, so this
# one does come from the build config header — meson's, whose location is
# fixed by the build directory rather than chosen by a configure script.
# Building with the gpl option off leaves HAVE_GPL either undefined or 0, and
# both pass.
MPV_CONFIG="$MPV_SRC/build/config.h"
if [[ ! -f "$MPV_CONFIG" ]]; then
  echo "cannot find mpv's config.h at $MPV_CONFIG; refusing to stage an unverified build" >&2
  exit 1
fi
if grep -qE '^#define HAVE_GPL 1' "$MPV_CONFIG"; then
  echo "mpv was built GPL; refusing to stage it" >&2
  exit 1
fi
echo "    mpv: HAVE_GPL 0"

# And nothing GPL linked in behind them. librubberband is the one that turns
# up by accident, because a package manager's mpv links it as a matter of
# course and it is GPL.
for lib in "$STAGE"/*; do
  [[ -f "$lib" ]] || continue
  case "$(basename "$lib")" in
    *rubberband*|*x264*|*x265*|*xvid*)
      echo "a GPL library reached the staging directory: $(basename "$lib")" >&2
      exit 1
      ;;
  esac
done

# ------------------------------------------------------------------- audio ---
#
# Read out of the library, for the same reason the licence is.
#
# mpv builds every audio output conditionally and says nothing when it builds
# none: the result plays video perfectly and is silent, which is a much harder
# thing to diagnose than a build that failed. So the finished library is asked
# what outputs it has, and a staging with nothing but `null` is refused.
echo "==> audio"
case "$(uname -s)" in
  Darwin) WANTED="coreaudio|avfoundation" ;;
  *)      WANTED="pulse|alsa|pipewire" ;;
esac

MPV_LIB=$(ls "$STAGE"/libmpv.* 2>/dev/null | head -1)
OUTPUTS=$(grep -a -oE "^($WANTED)$" "$MPV_LIB" 2>/dev/null | sort -u | tr '\n' ' ')
# Some platforms keep the names unanchored in the string table.
[[ -n "${OUTPUTS// /}" ]] || OUTPUTS=$(strings "$MPV_LIB" 2>/dev/null \
  | grep -xE "$WANTED" | sort -u | tr '\n' ' ')

if [[ -z "${OUTPUTS// /}" ]]; then
  echo "this libmpv has no audio output compiled in — it would play video and" >&2
  echo "nothing else. Install the development headers and build again:" >&2
  echo "  Linux:  libasound2-dev libpulse-dev libpipewire-0.3-dev" >&2
  echo "  macOS:  CoreAudio comes with the system; check the meson log" >&2
  exit 1
fi
echo "    audio outputs: $OUTPUTS"

ls -1 "$STAGE"
echo
echo "Staged LGPL libmpv and FFmpeg into $STAGE"

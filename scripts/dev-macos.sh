#!/bin/zsh
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Build it, install it, run it. For iterating.
#
# The difference from build-macos.sh is what it does *not* do: it never
# notarizes. Notarization is a round trip to Apple that has taken anywhere
# from forty seconds to an hour on a bad night, and it proves nothing about
# a build that is only going to be opened on this machine. Everything else
# is identical — same Developer ID signature, same hardened runtime, same
# entitlements — so a bug that only appears in a signed app still appears
# here. That matters: the LuaJIT crash was invisible in an unsigned build.
#
# It also replaces /Applications/Clicker.app, which is the point. A stale
# copy there looks exactly like the new one, and an evening can go into
# wondering why a change did not take when the answer is that the app being
# opened was built three commits ago.
#
#   ./scripts/dev-macos.sh          build, install, launch
#   ./scripts/dev-macos.sh --keep   build and install, but do not launch
#
# libmpv and FFmpeg are ours, built from source into third_party/mpv and
# bundled into Contents/Frameworks. Homebrew is never consulted: its FFmpeg is
# GPL, and an app that loads whatever the developer happens to have installed
# is not the app anybody else will run.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

LAUNCH=yes
[[ "${1:-}" == "--keep" ]] && LAUNCH=no

# The player, before the app that loads it.
#
# Nothing on macOS falls back to Homebrew any more — mpv_candidates in
# src/platform/macos.rs looks inside the bundle, then beside the executable,
# then in third_party, and stops. That is deliberate: Homebrew's FFmpeg is
# --enable-gpl and its mpv links librubberband, so a build that picked one up
# would be a GPL work inside an MIT one, and it would also be a build that
# only runs on machines with the same brew formulae installed.
#
# The consequence for iterating is that a tree without third_party/mpv
# produces an app that launches and cannot play anything. Rather than leave
# that as a surprise, build it here on the first run. It takes the better part
# of an hour once and is then cached until the pinned tags move.
if [[ ! -f "$ROOT/third_party/mpv/libmpv.2.dylib" ]]; then
  echo "==> no staged libmpv — building it first (once, then cached)"
  "$ROOT/scripts/build-mpv.sh"
fi

"$ROOT/scripts/build-macos.sh" --no-notarize --install

# One bundle on the machine, not two.
#
# macOS records a privacy decision — the local network one included — per
# app *location*, so a copy left in target/ and the copy in /Applications
# become two entries in System Settings, one of which is always the stale
# one somebody is about to toggle by mistake. The staging copy has already
# been zipped and installed by this point; keeping it buys nothing.
rm -rf "$ROOT/target/macos/Clicker.app"

if [[ "$LAUNCH" == "yes" ]]; then
  echo "==> launching"
  open /Applications/Clicker.app
fi

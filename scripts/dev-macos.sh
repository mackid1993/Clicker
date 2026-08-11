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

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

LAUNCH=yes
[[ "${1:-}" == "--keep" ]] && LAUNCH=no

"$ROOT/scripts/build-macos.sh" --no-notarize --install

if [[ "$LAUNCH" == "yes" ]]; then
  echo "==> launching"
  open /Applications/Clicker.app
fi

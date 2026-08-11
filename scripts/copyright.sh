#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial client for Channels DVR Server
# Copyright (c) 2026 David Brustein
#
# Print the licence that travels with an installed copy, covering everything
# in it rather than only the part that was written here.
#
# This exists because /usr/share/doc/clicker/copyright used to be LICENSE.md,
# which says MIT and nothing else — and what it accompanies is an MIT binary
# beside an LGPL player and an LGPL FFmpeg. Debian policy is explicit that the
# file must document all the licences of the package's contents, and the LGPL
# requires its notice to travel with the libraries. Both were satisfied only
# in spirit, by files further down the directory that nothing pointed at.
#
# Called by scripts/build-deb.sh and by `make install`, so the two cannot
# drift; the versions are read out of scripts/build-mpv.sh, so a version bump
# cannot leave this describing the previous one.
#
#   scripts/copyright.sh > /usr/share/doc/clicker/copyright

set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

tag() { sed -n "s/^$1=\"\(.*\)\"/\1/p" "$ROOT/scripts/build-mpv.sh"; }
MPV_V="$(tag MPV_TAG)"
FFMPEG_V="$(tag FFMPEG_TAG)"
LIBASS_V="$(tag LIBASS_TAG)"
LIBPLACEBO_V="$(tag LIBPLACEBO_TAG)"

for v in "$MPV_V" "$FFMPEG_V" "$LIBASS_V" "$LIBPLACEBO_V"; do
  if [ -z "$v" ]; then
    echo "copyright.sh: could not read the pinned versions from build-mpv.sh" >&2
    exit 1
  fi
done

cat "$ROOT/LICENSE.md"

cat <<COPYRIGHT

--------------------------------------------------------------------------

This package contains more than the program above. What is bundled with it,
and under what terms:

  mpv $MPV_V
      LGPL-2.1-or-later. libmpv.so.2, built with -Dgpl=false.
      https://mpv.io/

  FFmpeg $FFMPEG_V
      LGPL-2.1-or-later. libavcodec, libavformat, libavfilter, libavutil,
      libswscale, libswresample. Built with --disable-gpl and
      --disable-nonfree. https://ffmpeg.org/

  libass $LIBASS_V
      ISC. https://github.com/libass/libass

  libplacebo $LIBPLACEBO_V
      LGPL-2.1-or-later. https://code.videolan.org/videolan/libplacebo

  Fluent UI System Icons
      MIT, Microsoft Corporation. A subset, compiled into the binary.
      Full text in licenses/FluentSystemIcons-MIT.txt beside this file.

  Rust crates
      MIT or Apache-2.0. Listed in NOTICE.md beside this file.

The full text of the GNU Lesser General Public License, version 2.1, is in
licenses/LGPL-2.1.txt beside this file.

Its terms are met the way it intends. The LGPL libraries are shipped as
separate shared objects and are not linked into the executable, so you may
replace any of them with your own build of the same soname and this program
will load yours instead. To produce them from source, at the same versions
and with the same flags:

    git clone https://github.com/mackid1993/Clicker && cd Clicker
    ./scripts/build-mpv.sh

That script fetches each project at the version named above, builds it with
the GPL components disabled, and reads the licence back out of the finished
library before staging it. Nothing here is derived from a GPL build, which is
what those flags are for: this program is MIT, and combining it with GPL
components in a shipped binary would not be.

Channels DVR is the property of Fancy Bits, LLC. This program is an
unofficial client, contains no Channels code, and is not endorsed by,
sponsored by, or associated with them.
COPYRIGHT

#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Print the AppStream metainfo, which is what a software centre reads.
#
# Without this file GNOME Software opens the package and says "Unknown
# License — this app does not specify what license it is developed under, and
# may be proprietary", above "No details for this release", beside the package
# name in lower case and a generic gear. Every one of those is a true
# statement about metadata and a false one about the program: the licence has
# been in /usr/share/doc since the first package, and a software centre does
# not read /usr/share/doc. It reads this.
#
# Called by scripts/build-deb.sh and by `make install`, so the package and a
# source install describe themselves the same way.
#
#   scripts/metainfo.sh 1.2.1 > /usr/share/metainfo/<app-id>.metainfo.xml

set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_ID="io.github.mackid1993.Clicker"
VERSION="${1:-}"

if [ -z "$VERSION" ]; then
  echo "usage: $0 <version>" >&2
  exit 1
fi

# The release notes are the changelog's own words, so the two cannot disagree.
# Where the changelog has no section for the version being built — a test
# build, usually — the element is left out rather than filled with something
# invented.
NOTES=$(awk -v v="$VERSION" '
  $0 == "## " v { inside = 1; next }
  inside && /^## / { exit }
  inside && /^\*\*/ {
    line = $0
    sub(/^\*\*/, "", line)
    sub(/\*\*.*$/, "", line)
    gsub(/`/, "", line)
    gsub(/&/, "\\&amp;", line)
    gsub(/</, "\\&lt;", line)
    if (line != "") print "        <li>" line "</li>"
  }
' "$ROOT/CHANGELOG.md" | head -8)

cat <<METAINFO
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>$APP_ID</id>
  <metadata_license>CC0-1.0</metadata_license>
  <!-- The licence of this project's own source, which is what AppStream
       means by it and what a software centre renders: plain MIT, one
       identifier, recognised.
       It said "MIT AND LGPL-2.1-or-later" first, on the reasoning that the
       package is an MIT program and an LGPL player together — true of the
       package and not what this field asks. GNOME's own documentation names
       that mistake: software shows as proprietary or unknown when licences
       belonging to something else are folded in here. The bundled players'
       terms live in /usr/share/doc/clicker/copyright, which is the file that
       exists to carry them. -->
  <project_license>MIT</project_license>
  <name>Clicker</name>
  <summary>An Unofficial Client for Channels DVR Server</summary>
  <developer id="io.github.mackid1993">
    <name>David Brustein</name>
  </developer>
  <description>
    <p>
      A client for a Channels DVR server: live TV with a real guide,
      recordings, and downloads for watching away from the network.
    </p>
    <p>
      Playback is mpv, built LGPL and bundled, so nothing needs installing
      first. Graphics, hardware decoding and the cursor theme come from the
      machine rather than from the package.
    </p>
    <p>
      Not affiliated with, endorsed by or supported by Fancy Bits, LLC. It
      speaks to a Channels DVR server over its public HTTP API and contains
      no Channels code.
    </p>
  </description>
  <launchable type="desktop-id">$APP_ID.desktop</launchable>
  <!-- What binds this component to the installed package, and without which a
       software centre has a component and a package and no idea they are the
       same thing: it resolves the id, finds nothing installed under it, and
       leaves the application out of its Installed list entirely. Distribution
       metadata generators add this automatically; a metainfo file installed
       straight from a package has to say it. -->
  <pkgname>clicker</pkgname>
  <url type="homepage">https://github.com/mackid1993/Clicker</url>
  <url type="bugtracker">https://github.com/mackid1993/Clicker/issues</url>
  <categories>
    <category>AudioVideo</category>
    <category>Video</category>
    <category>TV</category>
  </categories>
  <content_rating type="oars-1.1"/>
  <releases>
    <release version="$VERSION" date="$(date -u +%Y-%m-%d)">
METAINFO

if [ -n "$NOTES" ]; then
  printf '      <description>\n        <ul>\n%s\n        </ul>\n      </description>\n' "$NOTES"
fi

cat <<METAINFO
    </release>
  </releases>
</component>
METAINFO

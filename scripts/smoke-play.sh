#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Play something for a minute and say, as a number, whether playback worked.
#
#   scripts/smoke-play.sh ~/test60.mp4
#   scripts/smoke-play.sh -s 90 http://dvr:8089/devices/ANY/channels/2.1/stream.mpg
#   scripts/smoke-play.sh -b /usr/bin/clicker ~/test60.mp4
#
# Why this exists. Every release until now has been playtested by a person
# watching a picture and forming an impression, on whichever machine was to
# hand, and the failure that costs a night is the one where every counter reads
# healthy and the picture is not — half the frames missing, a renderer
# reporting no error, a decoder keeping up perfectly. An impression cannot tell
# those apart and a number can: what matters is how many frames reached the
# screen against how many the decoder produced.
#
# The .ps1 beside this is the same test for Windows. Between them, one command
# per platform answers the question a release actually turns on.
#
# Exit status is the point: 0 if playback held up, 1 if it did not, 2 if the
# test could not be run at all. Nothing here is Linux-specific.

SECONDS_TO_PLAY=45
BINARY=""
SOURCE=""

while [ $# -gt 0 ]; do
  case "$1" in
    -s|--seconds) SECONDS_TO_PLAY="$2"; shift 2 ;;
    -b|--binary)  BINARY="$2"; shift 2 ;;
    -h|--help)    sed -n '8,20p' "$0"; exit 0 ;;
    *)            SOURCE="$1"; shift ;;
  esac
done

if [ -z "$SOURCE" ]; then
  echo "usage: $0 [-s seconds] [-b binary] <file-or-url>" >&2
  exit 2
fi

# Where the application keeps its log, which is where the numbers are. Same
# rule as platform::data_home, and it has to stay in step with it.
case "$(uname -s)" in
  Darwin) LOG="$HOME/Library/Application Support/Clicker/player.log" ;;
  Linux)  LOG="${XDG_DATA_HOME:-$HOME/.local/share}/Clicker/player.log" ;;
  *)      echo "smoke-play.sh does not know this system; use smoke-play.ps1 on Windows" >&2
          exit 2 ;;
esac

# The binary, if one was not named: a build in this tree first, because that is
# what somebody testing a change has just built, then what is installed.
if [ -z "$BINARY" ]; then
  ROOT="$(cd "$(dirname "$0")/.." && pwd)"
  for candidate in \
    "$ROOT/target/release/clicker" \
    "/Applications/Clicker.app/Contents/MacOS/clicker" \
    "/usr/bin/clicker" \
    "/usr/local/bin/clicker"
  do
    if [ -x "$candidate" ]; then BINARY="$candidate"; break; fi
  done
fi

if [ -z "$BINARY" ] || [ ! -x "$BINARY" ]; then
  echo "no clicker binary found; build one or pass -b <path>" >&2
  exit 2
fi

echo "==> $BINARY"
echo "==> playing $SOURCE for ${SECONDS_TO_PLAY}s"

# Everything written from here on is this run's. Counting the lines already
# there beats timestamps: a log is appended to by whatever else is running.
MARK=0
if [ -f "$LOG" ]; then MARK=$(wc -l < "$LOG" | tr -d ' '); fi

CLICKER_PLAY="$SOURCE" "$BINARY" >/dev/null 2>&1 &
PID=$!
sleep "$SECONDS_TO_PLAY"

if ! kill -0 "$PID" 2>/dev/null; then
  echo "FAIL: it exited on its own before the test was over" >&2
  exit 1
fi
kill "$PID" 2>/dev/null
sleep 2
kill -9 "$PID" 2>/dev/null

if [ ! -f "$LOG" ]; then
  echo "FAIL: no log at $LOG — did it start?" >&2
  exit 1
fi

# One line per five seconds, and three numbers out of each: frames drawn,
# what the decoder produced, and mpv's own running count of what it threw
# away. Portable sed rather than a gawk match(), because this runs on a Mac.
SAMPLES=$(tail -n "+$((MARK + 1))" "$LOG" \
  | sed -n 's/.*\] \([0-9.]*\)fps drawn.*decoder \([0-9.]*\)fps, mpv dropped \([0-9]*\).*/\1 \2 \3/p')

COUNT=$(printf '%s\n' "$SAMPLES" | grep -c '[0-9]')
if [ "$COUNT" -lt 3 ]; then
  echo "FAIL: only $COUNT measurements in ${SECONDS_TO_PLAY}s — playback never really started" >&2
  tail -n "+$((MARK + 1))" "$LOG" | grep -iE "error|refused|missing|could not" | head -5 >&2
  exit 1
fi

echo
printf '%s\n' "$SAMPLES" | awk '{printf "    drawn %6.1f   decoder %6.1f   mpv dropped %s\n", $1, $2, $3}'
echo

# The verdict. Frames drawn against frames decoded is the whole test: a
# renderer that cannot keep up shows here and nowhere else, because every
# other counter in the application stays healthy while it happens.
#
# The first sample is skipped — it covers the seconds where the stream was
# still opening — and 95% is the bar, which is loose enough that an ordinary
# dropped frame does not fail a build and tight enough that last night's
# regression, which ran at 75% of the decoder, would have.
printf '%s\n' "$SAMPLES" | awk -v secs="$SECONDS_TO_PLAY" '
  NR > 1 && $2 > 0 { drawn += $1; decoded += $2; n += 1; last = $3; if (first == "") first = $3 }
  END {
    if (n < 2) { print "FAIL: not enough usable measurements"; exit 1 }
    ratio = drawn / decoded
    grew  = last - first
    printf "    %.0f%% of decoded frames reached the screen (%d samples)\n", ratio * 100, n
    printf "    mpv dropped %d frames during the test\n", grew
    if (ratio < 0.95) {
      printf "\nFAIL: the renderer is not keeping up with the decoder.\n"
      exit 1
    }
    if (grew > secs / 10) {
      printf "\nFAIL: mpv is dropping frames steadily (%d in %ds).\n", grew, secs
      exit 1
    }
    print "\nPASS"
    exit 0
  }'

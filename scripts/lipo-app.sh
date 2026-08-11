#!/bin/bash
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Join two single-architecture Clicker.app bundles into one universal bundle.
#
#   ./scripts/lipo-app.sh <arm64.app> <x86_64.app> <output.app>
#
# The lazy way to be universal, and the only honest one here: each slice —
# the application, libmpv, FFmpeg, libass, libplacebo and everything they
# drag in — is compiled natively by a machine of its own architecture, and
# this joins the pairs afterwards. Cross-compiling the application alone
# produced a universal binary sitting beside arm64-only libraries, which is
# an Intel Mac launching and then failing to load its player.
#
# Signing happens after this, on the finished bundle, because a signature
# covers the bytes and these bytes did not exist until now.

set -euo pipefail

ARM_APP="${1:?first argument: the arm64 Clicker.app}"
INTEL_APP="${2:?second argument: the x86_64 Clicker.app}"
OUT_APP="${3:?third argument: where to write the universal Clicker.app}"

rm -rf "$OUT_APP"
mkdir -p "$(dirname "$OUT_APP")"
# The arm64 bundle is the template: same Info.plist, same icon, same
# licences, same layout. Only the Mach-O files differ between the two.
cp -R "$ARM_APP" "$OUT_APP"

join() {
  local arm="$1" intel="$2" out="$3"
  if [[ -f "$intel" ]]; then
    # Remove first. lipo opens its output through a symlink, so writing to
    # one would quietly overwrite whatever it points at instead.
    rm -f "$out"
    lipo -create "$arm" "$intel" -output "$out"
  else
    echo "    no x86_64 counterpart for $(basename "$arm") — leaving it single" >&2
  fi
}

echo "==> joining the executable"
join "$ARM_APP/Contents/MacOS/Clicker" \
     "$INTEL_APP/Contents/MacOS/Clicker" \
     "$OUT_APP/Contents/MacOS/Clicker"

echo "==> joining the libraries"
shopt -s nullglob
for arm_lib in "$ARM_APP/Contents/Frameworks/"*.dylib; do
  # Symlinks — libmpv.dylib pointing at libmpv.2.dylib and the like — are
  # left alone. Joining one would replace the link with a second copy of a
  # file the bundle already carries.
  [[ -L "$arm_lib" ]] && continue
  base="$(basename "$arm_lib")"
  join "$arm_lib" "$INTEL_APP/Contents/Frameworks/$base" \
       "$OUT_APP/Contents/Frameworks/$base"
done

# Anything the Intel bundle has and the arm64 one does not — a library a
# package manager only ships on one architecture — is carried over as-is
# rather than dropped silently.
for intel_lib in "$INTEL_APP/Contents/Frameworks/"*.dylib; do
  base="$(basename "$intel_lib")"
  if [[ ! -f "$OUT_APP/Contents/Frameworks/$base" ]]; then
    echo "    only x86_64 has $base; carrying it over" >&2
    cp "$intel_lib" "$OUT_APP/Contents/Frameworks/$base"
  fi
done

echo "==> what came out"
echo "    Clicker:  $(lipo -archs "$OUT_APP/Contents/MacOS/Clicker")"
for lib in "$OUT_APP/Contents/Frameworks/"*.dylib; do
  printf '    %-34s %s\n' "$(basename "$lib")" "$(lipo -archs "$lib")"
done

# A bundle whose libraries are not all universal is not universal, whatever
# the executable says.
#
# Every offender, not the first one — otherwise a bundle missing three
# libraries takes three CI runs to find that out. A plain string rather than
# an array because macOS still ships bash 3.2, where an empty array read
# under `set -u` is itself an error.
SINGLE=""
for lib in "$OUT_APP/Contents/Frameworks/"*.dylib; do
  archs="$(lipo -archs "$lib")"
  case "$archs" in
    *x86_64*) case "$archs" in *arm64*) continue ;; esac ;;
  esac
  SINGLE="$SINGLE    $(basename "$lib") [$archs]"$'\n'
done
if [[ -n "$SINGLE" ]]; then
  echo "these are not universal; refusing to call this bundle universal:" >&2
  printf '%s' "$SINGLE" >&2
  exit 1
fi

echo
echo "Universal bundle at $OUT_APP"

#!/bin/sh
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Remove a Clicker that was installed from source.
#
# Installed to $PREFIX/lib/clicker/uninstall.sh by `make install`, with the
# prefix substituted in, so it can be run long after the source tree is gone:
#
#   sudo /usr/local/lib/clicker/uninstall.sh
#
# That matters more than it sounds. `make uninstall` needs the checkout it was
# built from, and install.sh builds under a cache directory that anybody is
# entitled to delete — pointing somebody at a directory that may not exist is
# not an uninstall procedure.
#
# A package install is removed with `apt remove clicker` instead; this is only
# for the source path.

set -e

PREFIX="@PREFIX@"
APP_ID="io.github.mackid1993.Clicker"

case "$PREFIX" in
  ""|"@"*) echo "no prefix was baked into this script; refusing to guess" >&2; exit 1 ;;
  /*) ;;
  *) echo "prefix '$PREFIX' is not an absolute path; refusing" >&2; exit 1 ;;
esac

if [ ! -d "$PREFIX/lib/clicker" ] && [ ! -e "$PREFIX/bin/clicker" ]; then
  echo "Nothing installed under $PREFIX." >&2
  exit 1
fi

# Named individually rather than by wildcard. An uninstaller that composes
# paths from a variable is one empty variable away from removing /lib.
rm -rf "$PREFIX/lib/clicker"
rm -f  "$PREFIX/bin/clicker"
rm -f  "$PREFIX/share/applications/$APP_ID.desktop"
rm -f  "$PREFIX/share/icons/hicolor/512x512/apps/$APP_ID.png"
rm -rf "$PREFIX/share/doc/clicker"

command -v update-desktop-database >/dev/null 2>&1 \
  && update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 \
  && gtk-update-icon-cache -q -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true

echo "Clicker removed from $PREFIX."
echo
echo "Settings and downloads were left alone, deliberately:"
echo "  ${XDG_CONFIG_HOME:-$HOME/.config}/Clicker"
echo "  ${XDG_DATA_HOME:-$HOME/.local/share}/Clicker"
echo "Delete those yourself if you want them gone."

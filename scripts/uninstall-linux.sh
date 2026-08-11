#!/bin/sh
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial client for Channels DVR Server
# Copyright (c) 2026 David Brustein
#
# Remove a Clicker that was installed from source.
#
# Installed to $PREFIX/lib/clicker/uninstall.sh by `make install`, with the
# prefix substituted in, so it can be run long after the source tree is gone:
#
#   /usr/local/lib/clicker/uninstall.sh
#
# or from the applications menu, where `make install` also leaves an
# "Uninstall Clicker" entry pointing here.
#
# That it survives the source tree matters more than it sounds. `make
# uninstall` needs the checkout it was built from, and setup.sh builds under
# a cache directory anybody is entitled to delete — pointing somebody at a
# directory that may not exist is not an uninstall procedure.
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

# Ask for root rather than requiring it.
#
# Launched from the applications menu there is no shell to type sudo into, so
# the script elevates itself: pkexec puts up the desktop's own authentication
# dialog, and sudo is the fallback for a terminal. Doing it here rather than in
# the .desktop file means the command-line path gets it too.
if [ "$(id -u)" -ne 0 ]; then
  if   command -v pkexec >/dev/null 2>&1; then exec pkexec "$0" "$@"
  elif command -v sudo   >/dev/null 2>&1; then exec sudo   "$0" "$@"
  else echo "This needs root. Re-run it as root." >&2; exit 1
  fi
fi

if [ ! -d "$PREFIX/lib/clicker" ] && [ ! -e "$PREFIX/bin/clicker" ]; then
  echo "Nothing installed under $PREFIX." >&2
  exit 1
fi

# Whose settings to name at the end.
#
# Under pkexec or sudo, $HOME is root's, and telling somebody their settings
# are in /root/.config would be worse than saying nothing. Both tools say who
# called them, so ask.
CALLER_HOME="$HOME"
if [ -n "${PKEXEC_UID:-}" ]; then
  CALLER_HOME=$(getent passwd "$PKEXEC_UID" | cut -d: -f6)
elif [ -n "${SUDO_USER:-}" ]; then
  CALLER_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
fi
[ -n "$CALLER_HOME" ] || CALLER_HOME="$HOME"

# Named individually rather than by wildcard. An uninstaller that composes
# paths from a variable is one empty variable away from removing /lib.
rm -rf "$PREFIX/lib/clicker"
rm -f  "$PREFIX/bin/clicker"
rm -f  "$PREFIX/share/applications/$APP_ID.desktop"
rm -f  "$PREFIX/share/applications/$APP_ID.uninstall.desktop"
rm -f  "$PREFIX/share/icons/hicolor/512x512/apps/$APP_ID.png"
rm -rf "$PREFIX/share/doc/clicker"

command -v update-desktop-database >/dev/null 2>&1 \
  && update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 \
  && gtk-update-icon-cache -q -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true

echo "Clicker removed from $PREFIX."
echo
echo "Settings and downloads were left alone, deliberately:"
echo "  $CALLER_HOME/.config/Clicker"
echo "  $CALLER_HOME/.local/share/Clicker"
echo "Delete those yourself if you want them gone."

# Launched from the menu this runs in a terminal that closes when it exits,
# and a window that flashes and vanishes is indistinguishable from one that
# crashed. Only when there is nobody watching a shell.
if [ -t 0 ] && [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && [ -z "${SSH_TTY:-}" ]; then
  printf '\nPress Enter to close. '
  read -r _ || true
fi

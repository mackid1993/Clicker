#!/bin/bash
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Install Clicker on Linux, one line:
#
#   curl -fsSL https://raw.githubusercontent.com/mackid1993/Clicker/main/install.sh | bash
#
# On Debian, Ubuntu, Mint and Pop it offers two ways in and asks which you
# want: the .deb for this machine's architecture, which takes seconds and
# compiles nothing, or a build from source. Anywhere else — Fedora, Arch,
# openSUSE, anything — source is the only way, so it goes straight there.
#
# The source path installs the build dependencies, compiles FFmpeg, mpv and
# Clicker, and installs the result with its menu entry and icon. Twenty
# minutes to an hour, mostly FFmpeg.
#
#   ... | bash -s -- --uninstall      remove it again, either kind of install
#   ... | bash -s -- --from-source    skip the question, build from source
#   ... | bash -s -- --yes            skip every question, take the package
#   ... | bash -s -- --prefix=/opt    install somewhere other than /usr/local
#
# It says what it is about to do and waits for you to agree, which a script
# read off the internet ought to do. Nothing is written outside the package
# manager's control, $PREFIX, and a build directory it tells you about.

set -euo pipefail

REPO="mackid1993/Clicker"
PREFIX="/usr/local"
ASSUME_YES=no
FORCE_SOURCE=no
UNINSTALL=no
BUILD_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/clicker-build"

for argument in "$@"; do
  case "$argument" in
    --from-source) FORCE_SOURCE=yes ;;
    --uninstall)   UNINSTALL=yes ;;
    --yes|-y)      ASSUME_YES=yes ;;
    --prefix=*)    PREFIX="${argument#--prefix=}" ;;
    --help|-h)
      sed -n '6,24p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "unknown option: $argument" >&2; exit 2 ;;
  esac
done

say()  { printf '\033[1m==>\033[0m %s\n' "$*"; }
oops() { printf '\033[1;31mx\033[0m %s\n' "$*" >&2; exit 1; }

# Read from the terminal, not from stdin.
#
# Piped into bash, stdin *is* this script, so a plain `read` would swallow the
# next line of the program rather than wait for an answer.
confirm() {
  [[ "$ASSUME_YES" == "yes" ]] && return 0
  if [[ ! -t 0 && ! -r /dev/tty ]]; then
    oops "No terminal to ask on. Re-run with --yes if you meant it."
  fi
  printf '%s [y/N] ' "$1"
  local answer
  read -r answer < /dev/tty
  [[ "$answer" =~ ^[Yy] ]]
}

# Which of the two ways in, when both are open. Answers 1 or 2.
ask_method() {
  [[ "$ASSUME_YES" == "yes" ]] && { echo 1; return; }
  if [[ ! -t 0 && ! -r /dev/tty ]]; then echo 1; return; fi
  local answer
  while true; do
    printf '\nWhich would you like?\n' >&2
    printf '  1) Install the .deb package   — seconds, nothing compiled\n' >&2
    printf '  2) Build from source          — twenty minutes to an hour\n' >&2
    printf '\n[1/2] ' >&2
    read -r answer < /dev/tty
    case "${answer:-1}" in
      1|"") echo 1; return ;;
      2)    echo 2; return ;;
      *)    printf 'Type 1 or 2.\n' >&2 ;;
    esac
  done
}

[[ "$(uname -s)" == "Linux" ]] || oops "This installs Clicker on Linux. On macOS use the .app from the releases page; on Windows, the installer."
[[ $EUID -ne 0 ]] || oops "Do not run this as root. It asks for sudo where it needs it, and nowhere else."

command -v sudo >/dev/null || oops "sudo is needed to install packages."

# ------------------------------------------------------------- uninstall ---

# Both kinds of install, without needing to remember which one happened.
uninstall_everything() {
  local found=no

  if command -v dpkg >/dev/null && dpkg -s clicker >/dev/null 2>&1; then
    say "Removing the clicker package"
    sudo apt-get remove -y clicker
    found=yes
  fi

  # And a source install, wherever it went. Only the prefixes this script
  # offers, because guessing at others means guessing at paths to delete.
  local prefix
  for prefix in "$PREFIX" /usr/local /opt/clicker /usr; do
    if [[ -x "$prefix/lib/clicker/uninstall.sh" ]]; then
      say "Removing the source install under $prefix"
      sudo "$prefix/lib/clicker/uninstall.sh"
      found=yes
    fi
  done

  if [[ "$found" == "no" ]]; then
    oops "No Clicker found. If it was built from source with a PREFIX of its own, run: sudo <prefix>/lib/clicker/uninstall.sh"
  fi

  echo
  say "Done."
  echo "    Settings and downloads were left alone, deliberately:"
  echo "      ${XDG_CONFIG_HOME:-$HOME/.config}/Clicker"
  echo "      ${XDG_DATA_HOME:-$HOME/.local/share}/Clicker"
  exit 0
}

# `if`, not `&&`. Under `set -e` a test that comes out false is the whole
# command failing, and the script would exit here on every ordinary install.
if [[ "$UNINSTALL" == "yes" ]]; then
  uninstall_everything
fi

# ------------------------------------------------------- which machine is this ---

ID=""; ID_LIKE=""; PRETTY_NAME="unknown Linux"
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
fi
FAMILY="$ID $ID_LIKE"

case "$(uname -m)" in
  x86_64)         DEB_ARCH=amd64 ;;
  aarch64|arm64)  DEB_ARCH=arm64 ;;
  *)              DEB_ARCH="" ;;
esac

DEBIAN=no
case "$FAMILY" in *debian*|*ubuntu*) DEBIAN=yes ;; esac

echo
say "Clicker installer"
echo "    machine:      $PRETTY_NAME ($(uname -m))"

# --------------------------------------------------------------- the fast path ---

install_deb() {
  local url version
  say "Finding the latest release"
  # The releases API rather than a fixed URL, so this keeps working as
  # versions move. No token: public releases need none.
  url=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
        | grep -o "https://[^\"]*clicker_[^\"]*_${DEB_ARCH}\.deb" | head -1) || true
  [[ -n "$url" ]] || return 1

  version=$(basename "$url")
  echo "    package:      $version"
  echo "    installs to:  /usr (apt)"
  echo
  confirm "Download and install it?" || oops "Nothing was installed."

  local tmp="${TMPDIR:-/tmp}/$version"
  say "Downloading"
  curl -fL --progress-bar -o "$tmp" "$url"
  say "Installing"
  # apt rather than dpkg, so dependencies are resolved rather than reported.
  sudo apt-get install -y "$tmp"
  rm -f "$tmp"
  return 0
}

# ------------------------------------------------------------ the source path ---

install_from_source() {
  echo "    method:       build from source"
  echo "    installs to:  $PREFIX"
  echo "    builds in:    $BUILD_DIR"
  echo
  echo "    This compiles FFmpeg, mpv and Clicker. Twenty minutes to an hour,"
  echo "    depending on the machine, and it will ask sudo for build packages."
  echo
  confirm "Go ahead?" || oops "Nothing was installed."

  command -v git >/dev/null || {
    say "Installing git first"
    if   command -v apt-get >/dev/null; then sudo apt-get update && sudo apt-get install -y git
    elif command -v dnf     >/dev/null; then sudo dnf install -y git
    elif command -v pacman  >/dev/null; then sudo pacman -S --needed --noconfirm git
    elif command -v zypper  >/dev/null; then sudo zypper install -y git
    else oops "git is missing and I do not recognise this package manager."
    fi
  }

  if [[ -d "$BUILD_DIR/.git" ]]; then
    say "Updating $BUILD_DIR"
    git -C "$BUILD_DIR" fetch --depth 1 origin main
    git -C "$BUILD_DIR" reset --hard origin/main
  else
    say "Cloning into $BUILD_DIR"
    rm -rf "$BUILD_DIR"
    git clone --depth 1 "https://github.com/$REPO.git" "$BUILD_DIR"
  fi

  cd "$BUILD_DIR"
  say "Build dependencies"
  make deps

  # rustup drops cargo into ~/.cargo/bin, which the current shell does not
  # have on PATH yet if `make deps` is what installed it.
  [[ -r "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"
  command -v cargo >/dev/null || oops "cargo is still not on PATH. Open a new shell and re-run."

  say "Building (this is the long part)"
  make
  say "Installing to $PREFIX"
  sudo make install PREFIX="$PREFIX"
}

# ------------------------------------------------------------------ do it ---

if [[ "$FORCE_SOURCE" == "no" && "$DEBIAN" == "yes" && -n "$DEB_ARCH" ]]; then
  echo "    packages:     available for this machine"
  # Both ways work here, so the choice is not the script's to make. Building
  # from source on a Debian machine is a perfectly reasonable thing to want —
  # to read what you are installing, or to build against your own libraries —
  # and quietly taking the package away from someone who wanted that is rude.
  if [[ "$(ask_method)" == "1" ]]; then
    if install_deb; then
      echo
      say "Done. Clicker is in your menu, or run: clicker"
      exit 0
    fi
    echo
    say "No .deb published for $DEB_ARCH yet — building from source instead."
  fi
fi

install_from_source
echo
say "Done. Clicker is in your menu, or run: clicker"
echo "    Built in $BUILD_DIR — delete it if you want the space back."
echo "    Remove Clicker with: sudo $PREFIX/lib/clicker/uninstall.sh"

#!/bin/bash
# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial client for Channels DVR Server
# Copyright (c) 2026 David Brustein
#
# Install Clicker on Linux, one line:
#
#   curl -fsSL https://raw.githubusercontent.com/mackid1993/Clicker/main/setup.sh | bash
#
# It asks what you want to do, and offers what this machine can actually do:
# install the .deb, build from source, or uninstall what is already there.
# The package option appears on Debian, Ubuntu, Mint and Pop; anywhere else —
# Fedora, Arch, openSUSE, anything — source is the only way in. Uninstalling
# appears when there is something to uninstall.
#
# The source path installs the build dependencies, compiles FFmpeg, mpv and
# Clicker, and installs the result with its menu entry and icon. Twenty
# minutes to an hour, mostly FFmpeg.
#
#   ... | bash -s -- --uninstall      go straight to removing it
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

# What this run is for. Answers deb, source or uninstall.
#
# Everything the script can do, in one list, rather than a package/source
# question with removal hidden behind a flag somebody would have to already
# know about. The package option only appears where a package can be
# installed; uninstalling only appears when there is something to uninstall.
choose_action() {
  local can_deb="$1" installed="$2"
  local answer n
  local -a actions

  # `if`, not `&&`. A test that comes out false is a failed command, and under
  # `set -e` inside a case branch that ends the script rather than the branch —
  # the same trap that was in the uninstall dispatch.
  if [[ "$ASSUME_YES" == "yes" ]] || [[ ! -t 0 && ! -r /dev/tty ]]; then
    if [[ -n "$can_deb" ]]; then echo deb; else echo source; fi
    return
  fi

  while true; do
    # Numbered as they are printed, so the list always reads 1, 2, 3 — a menu
    # that starts at 2 because the first entry did not apply looks broken.
    # What each number means is therefore per-machine, which is why the answer
    # is looked up rather than assumed.
    actions=(unused)
    n=0

    printf '\nWhat would you like to do?\n' >&2
    if [[ -n "$can_deb" ]]; then
      n=$((n + 1)); actions+=(deb)
      printf '  %d) Install the .deb package   — seconds, nothing compiled\n' "$n" >&2
    fi
    n=$((n + 1)); actions+=(source)
    printf '  %d) Build from source          — twenty minutes to an hour\n' "$n" >&2
    if [[ "$installed" != "none" ]]; then
      n=$((n + 1)); actions+=(uninstall)
      printf '  %d) Uninstall Clicker          — currently %s\n' "$n" "$installed" >&2
    fi
    printf '  q) Quit\n' >&2
    printf '\n[1-%d, q] ' "$n" >&2

    read -r answer < /dev/tty
    case "$answer" in
      q|Q) echo quit; return ;;
      "")  echo "${actions[1]}"; return ;;
      *[!0-9]*|"") ;;
      *)
        if [[ "$answer" -ge 1 && "$answer" -le "$n" ]]; then
          echo "${actions[$answer]}"
          return
        fi
        ;;
    esac
    printf 'Not one of the choices.\n' >&2
  done
}

# What is on this machine already, said in a few words for the menu.
what_is_installed() {
  if command -v dpkg >/dev/null 2>&1 && dpkg -s clicker >/dev/null 2>&1; then
    echo "the $(dpkg-query -f '${Version}' -W clicker 2>/dev/null) package"
    return
  fi
  local prefix
  for prefix in "$PREFIX" /usr/local /opt/clicker /usr; do
    if [[ -x "$prefix/lib/clicker/uninstall.sh" ]]; then
      echo "built from source, under $prefix"
      return
    fi
  done
  echo none
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
      # It asks for root itself, so no sudo here — under sudo it would find
      # SUDO_USER and report the right home either way, but there is no need.
      "$prefix/lib/clicker/uninstall.sh"
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

INSTALLED="$(what_is_installed)"

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
  # PATH carried across sudo, because cargo from rustup lives in this user's
  # home and root has never heard of it. `make install` no longer compiles
  # anything, but the same is true of anything else the recipe shells out to.
  sudo env PATH="$PATH" make install PREFIX="$PREFIX"
}

# ------------------------------------------------------------------ do it ---

CAN_DEB=""
if [[ "$DEBIAN" == "yes" && -n "$DEB_ARCH" ]]; then
  CAN_DEB=yes
  echo "    packages:     available for this machine"
else
  echo "    packages:     none for this distribution — source is the way in"
fi
echo "    installed:    $INSTALLED"

# A flag settles it; otherwise ask.
if [[ "$FORCE_SOURCE" == "yes" ]]; then
  ACTION=source
else
  ACTION="$(choose_action "$CAN_DEB" "$INSTALLED")"
fi

case "$ACTION" in
  quit)
    say "Nothing was changed."
    exit 0
    ;;
  uninstall)
    uninstall_everything
    ;;
  deb)
    # Falls through to source when no package has been published for this
    # architecture yet, which is better than stopping at a dead end.
    if install_deb; then
      echo
      say "Done. Clicker is in your menu, or run: clicker"
      exit 0
    fi
    echo
    say "No .deb published for $DEB_ARCH yet — building from source instead."
    install_from_source
    ;;
  *)
    install_from_source
    ;;
esac
echo
say "Done. Clicker is in your menu, or run: clicker"
echo "    Built in $BUILD_DIR — delete it if you want the space back."
echo "    Remove Clicker with: $PREFIX/lib/clicker/uninstall.sh"
echo "    or \"Uninstall Clicker\" in your applications menu."

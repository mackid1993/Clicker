# SPDX-License-Identifier: MIT
#
# Clicker - an unofficial, native client for Channels DVR
# Copyright (c) 2026 David Brustein
#
# Building Clicker on Linux, for distributions the .deb does not cover.
#
#   make deps        install what is needed to build (asks for sudo)
#   make             build it
#   make install     install it, with its menu entry and icon
#   make uninstall   take it away again (or the uninstall.sh it installs,
#                    which keeps working once this directory is gone)
#   make deb         build a .deb instead of installing (Debian, Ubuntu)
#   make run         build and run without installing
#
# The whole thing, from nothing, on any distribution named below:
#
#   git clone https://github.com/mackid1993/Clicker && cd Clicker
#   make deps && make && sudo make install
#
# Build as yourself and install as root, in that order. `sudo make` would put
# root-owned files in your own target directory, and cargo installed by rustup
# is not on root's PATH at all.
#
# The long pole is `make deps` pulling packages and then FFmpeg and mpv
# compiling — twenty minutes to an hour depending on the machine. It happens
# once; `make` afterwards is a Rust build of a minute or two.
#
# Why mpv is built here rather than taken from the distribution: a package
# manager's FFmpeg is very often --enable-gpl, and its mpv links librubberband,
# which is GPL. Clicker is MIT. Combining those in a shipped binary is a
# licence violation, so scripts/build-mpv.sh builds both from pinned tags with
# --disable-gpl and -Dgpl=false and checks the result before staging it.

PREFIX  ?= /usr/local
DESTDIR ?=
APP_ID   = io.github.mackid1993.Clicker
VERSION  = $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

LIBDIR   = $(DESTDIR)$(PREFIX)/lib/clicker
BINDIR   = $(DESTDIR)$(PREFIX)/bin
APPSDIR  = $(DESTDIR)$(PREFIX)/share/applications
ICONDIR  = $(DESTDIR)$(PREFIX)/share/icons/hicolor/512x512/apps
DOCDIR   = $(DESTDIR)$(PREFIX)/share/doc/clicker

MPV_STAGE = third_party/mpv/libmpv.so.2

.PHONY: all build deps mpv install uninstall deb run clean help check-built

all: build

help:
	@sed -n '7,20p' Makefile | sed 's/^# \{0,1\}//'

# ------------------------------------------------------------------ build ---

# mpv first, because the application dlopens it and the staged copy is what
# gets installed beside the binary.
build: $(MPV_STAGE)
	cargo build --release

$(MPV_STAGE):
	./scripts/build-mpv.sh

mpv: $(MPV_STAGE)

run: build
	./target/release/clicker

# ------------------------------------------------------------------- deps ---

# Build dependencies, per package manager. Headers and build tools only —
# nothing here is shipped.
#
# libva and libvdpau are deliberately headers-only and never bundled: hardware
# decoding talks to the machine's own driver, so the machine's own libva has to
# be the one that loads. Same reason Mesa and libwayland are left alone.
#
# libass and libplacebo are asked for separately, with `|| true`, because they
# are the two build-mpv.sh will compile itself when the distribution's copy is
# too old — mpv 0.41 wants libass 0.17 and libplacebo 7.349. Installing the
# development packages lets pkg-config find a new enough one and skip that
# build entirely, which on a current distribution removes the whole autotools
# path. Where they are absent or too old, nothing is lost: the source build
# runs as before.
deps:
	@set -e; \
	if command -v apt-get >/dev/null; then \
	  echo "==> apt"; \
	  sudo apt-get update; \
	  sudo apt-get install -y build-essential git curl pkg-config \
	    meson ninja-build nasm patchelf python3-pip python3-setuptools \
	    autoconf automake libtool \
	    libgtk-3-dev libxdo-dev libayatana-appindicator3-dev \
	    libgl1-mesa-dev libfreetype6-dev libfribidi-dev libharfbuzz-dev \
	    libfontconfig1-dev libunibreak-dev libva-dev libvdpau-dev; \
	  sudo apt-get install -y libass-dev libplacebo-dev || true; \
	  meson --version 2>/dev/null | grep -qE '^0\.6[3-9]|^[1-9]' || sudo pip3 install --upgrade meson; \
	elif command -v dnf >/dev/null; then \
	  echo "==> dnf"; \
	  sudo dnf install -y gcc gcc-c++ make git curl pkgconf-pkg-config \
	    meson ninja-build nasm patchelf autoconf automake libtool \
	    gtk3-devel libxdo-devel libappindicator-gtk3-devel \
	    mesa-libGL-devel freetype-devel fribidi-devel harfbuzz-devel \
	    fontconfig-devel libunibreak-devel libva-devel libvdpau-devel; \
	  sudo dnf install -y libass-devel libplacebo-devel || true; \
	elif command -v pacman >/dev/null; then \
	  echo "==> pacman"; \
	  sudo pacman -S --needed --noconfirm base-devel git curl pkgconf \
	    meson ninja nasm patchelf autoconf automake libtool \
	    gtk3 xdotool libayatana-appindicator \
	    mesa freetype2 fribidi harfbuzz fontconfig libunibreak libva libvdpau; \
	  sudo pacman -S --needed --noconfirm libass libplacebo || true; \
	elif command -v zypper >/dev/null; then \
	  echo "==> zypper"; \
	  sudo zypper install -y -t pattern devel_basis; \
	  sudo zypper install -y git curl pkg-config meson ninja nasm patchelf \
	    autoconf automake libtool \
	    gtk3-devel xdotool-devel libayatana-appindicator3-devel \
	    Mesa-libGL-devel freetype2-devel fribidi-devel harfbuzz-devel \
	    fontconfig-devel libunibreak-devel libva-devel libvdpau-devel; \
	  sudo zypper install -y libass-devel libplacebo-devel || true; \
	else \
	  echo "No package manager I recognise. Install the equivalents of:" >&2; \
	  echo "  a C toolchain, git, curl, pkg-config, meson (>= 0.63), ninja," >&2; \
	  echo "  nasm, patchelf, autoconf, automake, libtool, and the" >&2; \
	  echo "  development headers for GTK 3, xdo," >&2; \
	  echo "  ayatana-appindicator, OpenGL, freetype, fribidi, harfbuzz," >&2; \
	  echo "  fontconfig, unibreak, libva and libvdpau." >&2; \
	  exit 1; \
	fi
	@command -v cargo >/dev/null || { \
	  echo "==> rust"; \
	  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; \
	  echo "Rust installed. Open a new shell, or: . \"$$HOME/.cargo/env\""; \
	}

# ---------------------------------------------------------------- install ---

# Say what is missing and how to get it, rather than building it here.
check-built:
	@[ -x target/release/clicker ] || { \
	  echo "target/release/clicker is not there yet." >&2; \
	  echo "Run \`make\` first — as yourself, not with sudo, so the build" >&2; \
	  echo "tree stays yours and cargo is the one on your PATH." >&2; \
	  exit 1; }
	@[ -f $(MPV_STAGE) ] || { \
	  echo "$(MPV_STAGE) is not there yet. Run \`make\` first." >&2; \
	  exit 1; }

# The binary goes beside its libraries and $(PREFIX)/bin holds a symlink to it.
#
# Not tidiness: the application finds libmpv by looking next to its own
# executable, and Linux resolves /proc/self/exe through the symlink, so both
# `clicker` typed at a shell and the desktop entry arrive at the real path with
# the player sitting right there.
# Deliberately not `install: build`.
#
# Installing needs root and building must not have it. Depending on build
# meant `sudo make install` ran cargo as root, which fails outright when cargo
# came from rustup — it lives in the invoking user's ~/.cargo/bin and root's
# PATH has never heard of it — and succeeds in a worse way when it does not,
# by leaving a target/ directory full of root-owned files that the next
# ordinary `cargo build` cannot write to.
#
# So: build as yourself, install as root. `make && sudo make install`.
install: check-built
	install -d $(LIBDIR) $(BINDIR) $(APPSDIR) $(ICONDIR) $(DOCDIR)/licenses
	install -m755 target/release/clicker $(LIBDIR)/clicker
	cp -a third_party/mpv/*.so* $(LIBDIR)/
	@# Anything libass needs that a machine cannot be assumed to have.
	@# libunibreak is the one that proved it necessary: the soname moved
	@# between Ubuntu releases, so libass found nothing to load and took
	@# libmpv down with it.
	@for lib in $(LIBDIR)/*.so*; do \
	  [ -f "$$lib" ] && [ ! -L "$$lib" ] || continue; \
	  ldd "$$lib" 2>/dev/null | awk '{print $$1, $$3}'; \
	done | sort -u | while read -r soname resolved; do \
	  case "$$soname" in libunibreak.so.*|libfribidi.so.*|libgraphite2.so.*) ;; *) continue ;; esac; \
	  [ -f "$$resolved" ] || continue; \
	  base=$$(basename "$$resolved"); \
	  [ -f "$(LIBDIR)/$$base" ] && continue; \
	  cp -L "$$resolved" "$(LIBDIR)/$$base" && chmod u+w "$(LIBDIR)/$$base"; \
	done
	@# $$ORIGIN is the loader's word for "the directory this file is in".
	@# Without it the bundled libraries look for each other at the path they
	@# were compiled at, which exists on no other machine.
	@command -v patchelf >/dev/null && for lib in $(LIBDIR)/*.so* $(LIBDIR)/clicker; do \
	  [ -f "$$lib" ] && [ ! -L "$$lib" ] && patchelf --set-rpath '$$ORIGIN' "$$lib" 2>/dev/null || true; \
	done || true
	ln -sf ../lib/clicker/clicker $(BINDIR)/clicker
	install -m644 assets/clicker.png $(ICONDIR)/$(APP_ID).png
	@# Icon= and the window's app_id are the same string on purpose: Wayland
	@# has no protocol for a window to carry its own icon, so the compositor
	@# matches app_id against installed desktop entries and uses what it
	@# finds there.
	printf '%s\n' \
	  '[Desktop Entry]' \
	  'Name=Clicker' \
	  'Comment=An unofficial client for Channels DVR' \
	  'Exec=clicker' \
	  'Icon=$(APP_ID)' \
	  'Terminal=false' \
	  'Type=Application' \
	  'Categories=AudioVideo;Video;TV;' \
	  'StartupWMClass=$(APP_ID)' \
	  > $(APPSDIR)/$(APP_ID).desktop
	install -m644 LICENSE.md NOTICE.md $(DOCDIR)/
	install -m644 licenses/* $(DOCDIR)/licenses/
	@# An uninstaller that outlives the source tree it came from.
	@#
	@# `make uninstall` needs this checkout, and install.sh builds under a
	@# cache directory anybody is entitled to delete. Pointing somebody at a
	@# directory that may not exist is not an uninstall procedure.
	sed 's|@PREFIX@|$(PREFIX)|' scripts/uninstall-linux.sh > $(LIBDIR)/uninstall.sh
	chmod 755 $(LIBDIR)/uninstall.sh
	-command -v update-desktop-database >/dev/null && update-desktop-database -q $(APPSDIR) 2>/dev/null
	-command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -f -t $(DESTDIR)$(PREFIX)/share/icons/hicolor 2>/dev/null
	@echo
	@echo "Clicker $(VERSION) installed. It is in your menu, or run: clicker"
	@echo "Remove it with:  sudo $(PREFIX)/lib/clicker/uninstall.sh"

uninstall:
	rm -rf $(LIBDIR)
	rm -f $(BINDIR)/clicker
	rm -f $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/$(APP_ID).png
	rm -rf $(DOCDIR)
	-command -v update-desktop-database >/dev/null && update-desktop-database -q $(APPSDIR) 2>/dev/null
	@echo "Clicker removed."

# ------------------------------------------------------------------- pack ---

deb: $(MPV_STAGE)
	./scripts/build-deb.sh

clean:
	cargo clean
	rm -rf target/deb

# third_party is deliberately not cleaned here. It holds FFmpeg and mpv, which
# take the better part of an hour to build and change only when their pinned
# tags do. `./scripts/build-mpv.sh --clean` is how you mean it.

#!/usr/bin/env sh
# repo-zoo installer for Linux.
#
# Usage:
#   ./scripts/install.sh               install into ~/.local
#   ./scripts/install.sh --system      install into /usr/local (needs write access)
#   ./scripts/install.sh --uninstall   remove the installed files
#   PREFIX=/opt/repo-zoo ./scripts/install.sh   install into a custom root
#
# Installs the binary, a .desktop entry, and an icon so the launcher shows up in
# the application menu and on the command line.
#
# When run from a release archive the prebuilt binary shipped next to this
# script is used and no Rust toolchain is required. When run from a repository
# checkout (no binary next to the script) a release build is performed first.
set -eu

BIN_NAME=repo-zoo
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT_DIR=$(dirname "$SCRIPT_DIR")
CARGO=${CARGO:-cargo}

PREFIX=${PREFIX:-}
SYSTEM=0
UNINSTALL=0

for arg in "$@"; do
    case "$arg" in
        --system) SYSTEM=1 ;;
        --uninstall) UNINSTALL=1 ;;
        *)
            echo "unknown argument: $arg" >&2
            exit 1
            ;;
    esac
done

if [ -z "$PREFIX" ]; then
    if [ "$SYSTEM" -eq 1 ]; then
        PREFIX=/usr/local
    else
        PREFIX=${HOME:-}/.local
    fi
fi

BIN_DIR=$PREFIX/bin
APP_DIR=$PREFIX/share/applications
ICON_DIR=$PREFIX/share/icons/hicolor/scalable/apps

uninstall() {
    echo "Removing repo-zoo from $PREFIX..."
    rm -f "$BIN_DIR/$BIN_NAME"
    rm -f "$APP_DIR/repo-zoo.desktop"
    rm -f "$ICON_DIR/repo-zoo.svg"
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$APP_DIR" 2>/dev/null || true
    fi
    echo "repo-zoo uninstalled."
}

if [ "$UNINSTALL" -eq 1 ]; then
    uninstall
    exit 0
fi

# A prebuilt binary shipped next to this script (release archive) skips the
# build entirely; a repository checkout has no such binary and builds a release
# instead.
if [ -x "$SCRIPT_DIR/$BIN_NAME" ]; then
    BINARY="$SCRIPT_DIR/$BIN_NAME"
else
    echo "Building $BIN_NAME (release)..."
    "$CARGO" build --release --manifest-path "$ROOT_DIR/Cargo.toml"
    BINARY="$ROOT_DIR/target/release/$BIN_NAME"
fi
if [ ! -x "$BINARY" ]; then
    echo "repo-zoo binary not found at $BINARY" >&2
    exit 1
fi

# The release archive ships the desktop entry and icon next to this script; the
# repository keeps them under packaging/.
find_asset() {
    if [ -f "$SCRIPT_DIR/$1" ]; then
        printf '%s\n' "$SCRIPT_DIR/$1"
    else
        printf '%s\n' "$ROOT_DIR/packaging/$1"
    fi
}
DESKTOP=$(find_asset repo-zoo.desktop)
SVG=$(find_asset repo-zoo.svg)

echo "Installing into $PREFIX..."
mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"
cp "$BINARY" "$BIN_DIR/$BIN_NAME"
cp "$DESKTOP" "$APP_DIR/repo-zoo.desktop"
cp "$SVG" "$ICON_DIR/repo-zoo.svg"
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR" 2>/dev/null || true
fi

echo
echo "Installed. Run with: $BIN_DIR/$BIN_NAME"
echo "On first run it creates ~/.config/repo-zoo/config.toml (seeded from a"
echo "scan of ~/code, or your home directory)."
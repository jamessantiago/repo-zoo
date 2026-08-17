#!/usr/bin/env sh
# repo-zoo installer for Linux.
#
# Usage:
#   ./scripts/install.sh               build + install into ~/.local
#   ./scripts/install.sh --system      install into /usr/local (needs write access)
#   ./scripts/install.sh --uninstall   remove the installed files
#   PREFIX=/opt/repo-zoo ./scripts/install.sh   install into a custom root
#
# Installs the release binary, a .desktop entry, and an icon so the launcher
# shows up in the application menu and on the command line.
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

echo "Building $BIN_NAME (release)..."
"$CARGO" build --release --manifest-path "$ROOT_DIR/Cargo.toml"
BINARY="$ROOT_DIR/target/release/$BIN_NAME"
if [ ! -x "$BINARY" ]; then
    echo "build did not produce $BINARY" >&2
    exit 1
fi

echo "Installing into $PREFIX..."
mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"
cp "$BINARY" "$BIN_DIR/$BIN_NAME"
cp "$ROOT_DIR/packaging/repo-zoo.desktop" "$APP_DIR/repo-zoo.desktop"
cp "$ROOT_DIR/packaging/repo-zoo.svg" "$ICON_DIR/repo-zoo.svg"
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR" 2>/dev/null || true
fi

echo
echo "Installed. Run with: $BIN_DIR/$BIN_NAME"
echo "On first run it creates ~/.config/repo-zoo/config.toml (seeded from a"
echo "scan of ~/code, or your home directory)."
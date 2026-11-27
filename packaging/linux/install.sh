#!/bin/sh
# Builds (if needed) and installs Glance for the current user on Linux.
#   ./packaging/linux/install.sh            # from the project folder
#   ./install.sh                            # from an extracted release with ./glance
# Uninstall:  ./install.sh --uninstall
set -eu

APP_ID=io.github.glance.Glance
BIN_DIR="${HOME}/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
HERE="$(cd "$(dirname "$0")" && pwd)"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN_DIR/glance" "$DATA_DIR/applications/$APP_ID.desktop"
    for s in 48 128 256; do rm -f "$DATA_DIR/icons/hicolor/${s}x${s}/apps/$APP_ID.png"; done
    rm -rf "$DATA_DIR/glance"
    command -v update-desktop-database >/dev/null && update-desktop-database "$DATA_DIR/applications" || true
    echo "Glance removed."
    exit 0
fi

# Find a binary: next to this script (release archive) or build from source.
if [ -x "$HERE/glance" ]; then
    BIN="$HERE/glance"
    DESKTOP="$HERE/$APP_ID.desktop"
else
    ROOT="$(cd "$HERE/../.." && pwd)"
    command -v cargo >/dev/null || { echo "Rust is required: https://rustup.rs" >&2; exit 1; }
    (cd "$ROOT" && cargo build --release)
    BIN="${CARGO_TARGET_DIR:-$ROOT/target}/release/glance"
    DESKTOP="$HERE/$APP_ID.desktop"
fi

mkdir -p "$BIN_DIR" "$DATA_DIR/applications"
install -m 755 "$BIN" "$BIN_DIR/glance"
install -m 644 "$DESKTOP" "$DATA_DIR/applications/$APP_ID.desktop"
for s in 48 128 256; do
    d="$DATA_DIR/icons/hicolor/${s}x${s}/apps"
    mkdir -p "$d"
    "$BIN_DIR/glance" --export-icon "$d/$APP_ID.png" "$s"
done
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA_DIR/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$DATA_DIR/icons/hicolor" || true

echo "Glance installed to $BIN_DIR/glance"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "Note: add $BIN_DIR to your PATH to run 'glance' from a terminal." ;; esac
echo "To make it the default:  xdg-mime default $APP_ID.desktop image/png image/jpeg image/gif image/webp"

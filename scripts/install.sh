#!/usr/bin/env bash
set -euo pipefail

# Installs the Rustify binary and desktop integration files for the current user.
# Usage:
#   ./scripts/install.sh [path-to-rustify-binary]
# Defaults to target/release/rustify.

BINARY="${1:-target/release/rustify}"
PREFIX="${HOME}/.local"

if [[ ! -f "$BINARY" ]]; then
    echo "Binary not found: $BINARY" >&2
    echo "Build it first with: cargo build --release" >&2
    exit 1
fi

mkdir -p \
    "$PREFIX/bin" \
    "$PREFIX/share/applications" \
    "$PREFIX/share/icons/hicolor/scalable/apps"

install -m755 "$BINARY" "$PREFIX/bin/rustify"

# Raster icons.
for size in 16 32 48 64 128 256; do
    src="packaging/icons/hicolor/${size}x${size}/apps/rustify.png"
    if [[ -f "$src" ]]; then
        mkdir -p "$PREFIX/share/icons/hicolor/${size}x${size}/apps"
        install -m644 "$src" "$PREFIX/share/icons/hicolor/${size}x${size}/apps/rustify.png"
    fi
done

# Scalable icon.
if [[ -f "packaging/icons/hicolor/scalable/apps/rustify.svg" ]]; then
    install -m644 \
        "packaging/icons/hicolor/scalable/apps/rustify.svg" \
        "$PREFIX/share/icons/hicolor/scalable/apps/rustify.svg"
fi

install -m644 "packaging/linux/rustify.desktop" \
    "$PREFIX/share/applications/rustify.desktop"

# Refresh caches when the tools are available.
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "Rustify installed to $PREFIX/bin/rustify"
echo "You may need to log out and back in for the menu entry to appear."
#!/usr/bin/env bash
set -euo pipefail

# Installs Rustify binary and desktop integration files for the current user.
# Usage:
#   ./scripts/install.sh [path-to-rustify-binary]
#
# Works both from the repository root and from an extracted release archive.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${HOME}/.local"

# Resolve binary: explicit arg, then ./rustify next to script, then repo target.
if [[ $# -ge 1 ]]; then
    BINARY="$1"
elif [[ -f "$SCRIPT_DIR/rustify" ]]; then
    BINARY="$SCRIPT_DIR/rustify"
elif [[ -f "$SCRIPT_DIR/../target/release/rustify" ]]; then
    BINARY="$SCRIPT_DIR/../target/release/rustify"
else
    BINARY="target/release/rustify"
fi

# Resolve packaging dir: next to script, or repo packaging/.
if [[ -d "$SCRIPT_DIR/packaging" ]]; then
    PKG="$SCRIPT_DIR/packaging"
elif [[ -d "$SCRIPT_DIR/../packaging" ]]; then
    PKG="$SCRIPT_DIR/../packaging"
else
    PKG="packaging"
fi

if [[ ! -f "$BINARY" ]]; then
    echo "Binary not found: $BINARY" >&2
    echo "Build it first with: cargo build --release, or pass the binary path." >&2
    exit 1
fi

mkdir -p \
    "$PREFIX/bin" \
    "$PREFIX/share/applications" \
    "$PREFIX/share/icons/hicolor/scalable/apps"

install -m755 "$BINARY" "$PREFIX/bin/rustify"

# Install bundled yt-dlp if present next to binary.
BINARY_DIR="$(cd "$(dirname "$BINARY")" && pwd)"
if [[ -f "$BINARY_DIR/yt-dlp" ]]; then
    install -m755 "$BINARY_DIR/yt-dlp" "$PREFIX/bin/yt-dlp"
    echo "Installed bundled yt-dlp to $PREFIX/bin/yt-dlp"
fi

# Raster icons.
for size in 16 32 48 64 128 256; do
    src="$PKG/icons/hicolor/${size}x${size}/apps/rustify.png"
    if [[ -f "$src" ]]; then
        mkdir -p "$PREFIX/share/icons/hicolor/${size}x${size}/apps"
        install -m644 "$src" "$PREFIX/share/icons/hicolor/${size}x${size}/apps/rustify.png"
    fi
done

# Scalable icon.
if [[ -f "$PKG/icons/hicolor/scalable/apps/rustify.svg" ]]; then
    install -m644 \
        "$PKG/icons/hicolor/scalable/apps/rustify.svg" \
        "$PREFIX/share/icons/hicolor/scalable/apps/rustify.svg"
fi

if [[ -f "$PKG/linux/rustify.desktop" ]]; then
    install -m644 "$PKG/linux/rustify.desktop" \
        "$PREFIX/share/applications/rustify.desktop"
fi

# Refresh caches when the tools are available.
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "Rustify installed to $PREFIX/bin/rustify"
echo "Make sure libmpv is installed on your system (see README)."
echo "You may need to log out and back in for the menu entry to appear."
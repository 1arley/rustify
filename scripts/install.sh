#!/usr/bin/env bash
set -euo pipefail

# Installs Rustify binary and desktop integration files for the current user.
# Usage:
#   ./scripts/install.sh [--symlink] [path-to-rustify-binary]
#
# Options:
#   --symlink, -l  Symlink the binary instead of copying it. Useful for dev
#                  builds so the installed command tracks target/release/rustify.
#
# Works both from the repository root and from an extracted release archive.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${HOME}/.local"
SYMLINK=false
BINARY_ARG=""

for arg in "$@"; do
    case "$arg" in
        --symlink|-l) SYMLINK=true ;;
        -h|--help)
            sed -n '1,20p' "$0"
            exit 0
            ;;
        *)
            BINARY_ARG="$arg"
            ;;
    esac
done

# Resolve binary: explicit arg, then ./rustify next to script, then repo target.
if [[ -n "$BINARY_ARG" ]]; then
    BINARY="$BINARY_ARG"
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

BINARY_DIR="$(cd "$(dirname "$BINARY")" && pwd)"

# Install bundled yt-dlp if present next to binary.
if [[ -f "$BINARY_DIR/yt-dlp" ]]; then
    install -m755 "$BINARY_DIR/yt-dlp" "$PREFIX/bin/yt-dlp"
    echo "Installed bundled yt-dlp to $PREFIX/bin/yt-dlp"
fi

# If this is a packaged release with a wrapper + bundled libs, install them properly.
if [[ -f "$BINARY_DIR/rustify.bin" ]]; then
    install -m755 "$BINARY_DIR/rustify.bin" "$PREFIX/bin/rustify.bin"
    if [[ -d "$BINARY_DIR/lib" ]]; then
        mkdir -p "$PREFIX/lib/rustify"
        cp -a "$BINARY_DIR/lib/." "$PREFIX/lib/rustify/"
    fi
    cat > "$PREFIX/bin/rustify" << 'WRAP'
#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export LD_LIBRARY_PATH="$DIR/../lib/rustify${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$DIR/rustify.bin" "$@"
WRAP
    chmod +x "$PREFIX/bin/rustify"
else
    if [[ "$SYMLINK" == true ]]; then
        BINARY_ABS="$(cd "$(dirname "$BINARY")" && pwd)/$(basename "$BINARY")"
        ln -sf "$BINARY_ABS" "$PREFIX/bin/rustify"
        echo "Symlinked $PREFIX/bin/rustify -> $BINARY_ABS"
    else
        install -m755 "$BINARY" "$PREFIX/bin/rustify"
    fi
    echo "Make sure libmpv is installed on your system (see README)."
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
echo "You may need to log out and back in for the menu entry to appear."
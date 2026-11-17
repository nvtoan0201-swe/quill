#!/bin/sh
# Build and install Quill for the current user (~/.local).
set -e
cd "$(dirname "$0")/.."
cargo build --release
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 target/release/quill "$PREFIX/bin/quill"
install -Dm644 packaging/quill.desktop "$PREFIX/share/applications/quill.desktop"
install -Dm644 packaging/quill.png "$PREFIX/share/icons/hicolor/512x512/apps/quill.png"
for size in 256 128 64 48; do
  if command -v convert >/dev/null 2>&1; then
    mkdir -p "$PREFIX/share/icons/hicolor/${size}x${size}/apps"
    convert packaging/quill.png -resize "${size}x${size}" "$PREFIX/share/icons/hicolor/${size}x${size}/apps/quill.png"
  fi
done
rm -f "$PREFIX/share/icons/hicolor/scalable/apps/quill.svg"
gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
echo "Installed to $PREFIX/bin/quill"

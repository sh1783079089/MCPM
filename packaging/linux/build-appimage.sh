#!/usr/bin/env bash
# Build an AppImage for the mcpm CLI.
# usage: build-appimage.sh <binary> <version> <outdir>
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <binary> <version> <outdir>" >&2
  exit 1
fi

BIN="$(readlink -f "$1")"
VERSION="$2"
OUT="$(readlink -fm "$3")"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ASSETS_DIR="$SCRIPT_DIR/../appimage"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

APPDIR="$WORK/AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/icons/hicolor/128x128/apps"

install -m 0755 "$BIN" "$APPDIR/usr/bin/mcpm"
install -m 0755 "$ASSETS_DIR/AppRun" "$APPDIR/AppRun"
install -m 0644 "$ASSETS_DIR/mcpm.desktop" "$APPDIR/mcpm.desktop"
install -m 0644 "$ASSETS_DIR/mcpm.png" "$APPDIR/mcpm.png"
install -m 0644 "$ASSETS_DIR/mcpm.png" "$APPDIR/usr/share/icons/hicolor/128x128/apps/mcpm.png"
ln -srf "$APPDIR/mcpm.png" "$APPDIR/.DirIcon"

TOOL="$WORK/appimagetool.AppImage"
curl -fsSL "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" -o "$TOOL"
chmod +x "$TOOL"

APPIMAGE="$OUT/mcpm-${VERSION}-linux-x86_64.AppImage"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" "$APPDIR" "$APPIMAGE"

# Verify the AppImage runs without FUSE.
output="$(APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGE" --version)"
case "$output" in
  mcpm*) ;;
  *) echo "unexpected --version output: $output" >&2; exit 1 ;;
esac

echo "$APPIMAGE"

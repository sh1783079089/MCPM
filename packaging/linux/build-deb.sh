#!/usr/bin/env bash
# Build a .deb package for the mcpm CLI.
# usage: build-deb.sh <binary> <version> <outdir>
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <binary> <version> <outdir>" >&2
  exit 1
fi

BIN="$(readlink -f "$1")"
RAW_VERSION="$2"
OUT="$(readlink -fm "$3")"
mkdir -p "$OUT"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LICENSE_PATH="$SCRIPT_DIR/../../LICENSE"

# Debian pre-release marker: 0.1.0-rc1 -> 0.1.0~rc1
DEB_VERSION="${RAW_VERSION/#v/}"
DEB_VERSION="${DEB_VERSION//-/\~}"

compute_depends() {
  local so pkg deps=""
  while read -r so; do
    [ -z "$so" ] && continue
    pkg="$(dpkg -S "$so" 2>/dev/null | head -n 1 | cut -d: -f1 || true)"
    [ -z "$pkg" ] && continue
    case "$pkg" in
      libssl3t64) pkg="libssl3t64 | libssl3" ;;
      libssl3)    pkg="libssl3 | libssl3t64" ;;
    esac
    case ", $deps," in
      *", $pkg,"*) continue ;;
    esac
    deps="${deps:+$deps, }$pkg"
  done < <(objdump -p "$BIN" | sed -n 's/.*NEEDED *//p' | sort -u)
  echo "${deps:-libc6}"
}

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PKG="$WORK/mcpm"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/usr/share/doc/mcpm"

install -m 0755 "$BIN" "$PKG/usr/bin/mcpm"
install -m 0644 "$LICENSE_PATH" "$PKG/usr/share/doc/mcpm/copyright"

cat > "$PKG/DEBIAN/control" <<EOF
Package: mcpm
Version: $DEB_VERSION
Section: utils
Priority: optional
Architecture: amd64
Depends: $(compute_depends)
Maintainer: mcpm contributors
Description: Minecraft modpack manager
 Reproducible Minecraft modpack management with dependency
 resolution, environment solving and compatibility checks.
EOF

DEB="$OUT/mcpm_${DEB_VERSION}_amd64.deb"
dpkg-deb --build --root-owner-group "$PKG" "$DEB"
dpkg-deb --info "$DEB" >/dev/null

echo "$DEB"

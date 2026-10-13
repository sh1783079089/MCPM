#!/usr/bin/env bash
# Build an .rpm package for the mcpm CLI.
# usage: build-rpm.sh <binary> <version> <outdir>
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
LICENSE_PATH="$(readlink -f "$SCRIPT_DIR/../../LICENSE")"

# RPM pre-release marker: 0.1.0-rc1 -> 0.1.0~rc1
RPM_VERSION="${RAW_VERSION/#v/}"
RPM_VERSION="${RPM_VERSION//-/\~}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

sed \
  -e "s|@VERSION@|$RPM_VERSION|g" \
  -e "s|@BINPATH@|$BIN|g" \
  -e "s|@LICENSEPATH@|$LICENSE_PATH|g" \
  "$SCRIPT_DIR/mcpm.spec.in" > "$WORK/SPECS/mcpm.spec"

rpmbuild \
  --define "_topdir $WORK" \
  --define "binpath $BIN" \
  --define "licensepath $LICENSE_PATH" \
  -bb "$WORK/SPECS/mcpm.spec"

shopt -s nullglob
RPMS=("$WORK"/RPMS/*/*.rpm)
shopt -u nullglob

if [ "${#RPMS[@]}" -eq 0 ]; then
  echo "no rpm produced" >&2
  exit 1
fi

cp "${RPMS[@]}" "$OUT/"
printf '%s\n' "${RPMS[@]}"

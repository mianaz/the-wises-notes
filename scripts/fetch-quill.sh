#!/usr/bin/env bash
# Fetch libquill.so, quill's clean-room MIT e-ink adapter
# (https://github.com/MaximeRivest/quill), from riddle's public v0.3.0
# release, where it ships prebuilt. The archive is checked against a pinned
# SHA-256, and only libquill.so is kept.
#
#   ./scripts/fetch-quill.sh          # writes $SAGE_LINK_DIR/libquill.so if absent
#   ./scripts/fetch-quill.sh --force  # fetch again
set -euo pipefail
cd "$(dirname "$0")/.."

LINK_DIR=${SAGE_LINK_DIR:-$HOME/sage-link}
URL=https://github.com/MaximeRivest/riddle/releases/download/v0.3.0/riddle-0.3.0.zip
ZIP_SHA=1f37eac57183b664148b226fe285d04f75d9d43d4e5edfc532be9ae6b57e5461
LIB_SHA=e395af8e7ffb614dedf4370ef5fa2acc5ffa1f195da415b9049fd4f7d96f9929

if [ "${1:-}" != --force ] && [ -f "$LINK_DIR/libquill.so" ]; then
    exit 0
fi

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
echo "fetching libquill.so from riddle v0.3.0"
curl -fsSL "$URL" -o "$T/riddle.zip"
echo "$ZIP_SHA  $T/riddle.zip" | sha256sum -c --quiet - \
    || { echo "checksum mismatch for $URL" >&2; exit 1; }
python3 -I -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extract("libquill.so", sys.argv[2])' \
    "$T/riddle.zip" "$T"
echo "$LIB_SHA  $T/libquill.so" | sha256sum -c --quiet -
mkdir -p "$LINK_DIR"
cp "$T/libquill.so" "$LINK_DIR/libquill.so"
echo "libquill.so -> $LINK_DIR"

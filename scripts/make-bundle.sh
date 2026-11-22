#!/usr/bin/env bash
# Stage the AppLoad bundle and zip it as a release.
#   ./scripts/make-bundle.sh           # after ./scripts/build.sh
#   ./scripts/make-bundle.sh --stage   # stage only (used by deploy.sh)
#
# dist/bundle/sage-notes      the Chinese tile: binary, libraries, fonts, persona…
# dist/bundle/sage-notes-en   the English tile: a launcher reusing the folder above
# dist/sage-notes-<version>.zip  both folders, ready to copy into AppLoad
#
# The zip never contains oracle.env (your key) or canon.txt (HoYoverse's text).
set -euo pipefail
cd "$(dirname "$0")/.."

ZIP=1
for a in "$@"; do
    case "$a" in
        --stage) ZIP=0 ;;
        *) echo "usage: $0 [--stage]" >&2; exit 2 ;;
    esac
done

for f in dist/sage dist/libquill.so dist/libquillcompat.so; do
    [ -f "$f" ] || { echo "missing $f: run ./scripts/build.sh first" >&2; exit 1; }
done
for f in dist/sage dist/libquillcompat.so; do
    if LC_ALL=C grep -aqF -e "$HOME" -e "$PWD" "$f"; then
        echo "$f contains a local build path: rebuild with ./scripts/build.sh" >&2
        exit 1
    fi
done
[ -f fonts/LXGWWenKai-Regular.ttf ] || ./scripts/fetch-fonts.sh

VERSION=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)
B=dist/bundle
rm -rf "$B"
A="$B/sage-notes" E="$B/sage-notes-en"
mkdir -p "$A/fonts" "$E"

cp dist/sage dist/libquill.so dist/libquillcompat.so "$A/"
cp app/external.manifest.json app/appload-launch.sh app/sage-takeover.sh app/sage-cli.sh \
   app/persona.txt app/canon.example.txt app/oracle.env.example app/settings.schema.json \
   app/icon.png "$A/"
cp fonts/LXGWWenKai-Regular.ttf fonts/CrimsonPro-Italic.ttf fonts/OFL-*.txt "$A/fonts/"
cp LICENSE ACKNOWLEDGEMENTS.md "$A/"
cp licenses/quill-LICENSE.txt "$A/LICENSE-quill.txt"

cp app/en/external.manifest.json app/en/appload-launch.sh app/icon.png "$E/"

# The tablet runs Linux: LF endings, executable launchers.
find "$B" -type f \( -name '*.sh' -o -name '*.txt' -o -name '*.json' -o -name '*.md' -o -name '*.example' \) \
    -exec sed -i 's/\r$//' {} +
chmod 755 "$A/sage" "$A"/*.sh "$E"/*.sh
chmod 644 "$A"/*.so

echo "staged: $A, $E"
[ "$ZIP" = 1 ] || exit 0

OUT="dist/sage-notes-$VERSION.zip"
rm -f "$OUT"
# Unix modes are set here, not taken from disk: a Windows drive mounted in WSL
# reports 777 for everything. The binary and launchers are 755, the rest 644.
python3 -I - "$B" "$OUT" <<'PY'
import os, sys, zipfile
root, out = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for d, _, files in sorted(os.walk(root)):
        for name in sorted(files):
            p = os.path.join(d, name)
            info = zipfile.ZipInfo.from_file(p, os.path.relpath(p, root))
            info.compress_type = zipfile.ZIP_DEFLATED
            mode = 0o755 if name == "sage" or name.endswith(".sh") else 0o644
            info.external_attr = (0o100000 | mode) << 16
            with open(p, "rb") as f:
                z.writestr(info, f.read())
PY
(cd dist && sha256sum "$(basename "$OUT")" | tee "$(basename "$OUT").sha256")
echo "release: $OUT ($(du -h "$OUT" | cut -f1))"

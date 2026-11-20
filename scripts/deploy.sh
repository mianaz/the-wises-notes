#!/usr/bin/env bash
# Install or update the 贤者笔记 AppLoad bundle on the tablet.
#   ./scripts/deploy.sh            # after ./scripts/build.sh
#   ./scripts/deploy.sh --canon    # also copy app/canon.txt to the tablet
# The device keeps its own oracle.env (API key) and canon.txt; this script
# never overwrites them unless --canon asks it to copy your local canon.txt
# (HoYoverse's script text, for your own personal use only: it goes to your
# own tablet and nowhere else). On first install it seeds oracle.env from
# riddle's, renaming RIDDLE_* keys to SAGE_*.
set -euo pipefail
cd "$(dirname "$0")/.."
PUSH_CANON=0
for a in "$@"; do
    case "$a" in
        --canon) PUSH_CANON=1 ;;
        *) echo "usage: $0 [--canon]" >&2; exit 2 ;;
    esac
done
if [ "$PUSH_CANON" = 1 ] && [ ! -f app/canon.txt ]; then
    echo "no app/canon.txt: run ./scripts/fetch-canon.py first" >&2; exit 1
fi
HOST=${SAGE_HOST:-rm}
APP=/home/root/xovi/exthome/appload/sage-notes

[ -f dist/sage ] || { echo "build first: ./scripts/build.sh" >&2; exit 1; }
[ -f fonts/LXGWWenKai-Regular.ttf ] || ./scripts/fetch-fonts.sh

S=$(mktemp -d)
trap 'rm -rf "$S"' EXIT
mkdir -p "$S/fonts"
cp dist/sage "$S/"
cp app/external.manifest.json app/appload-launch.sh app/sage-takeover.sh app/sage-cli.sh app/persona.txt \
   app/canon.example.txt app/oracle.env.example app/settings.schema.json app/icon.png "$S/"
cp fonts/LXGWWenKai-Regular.ttf fonts/CrimsonPro-Italic.ttf fonts/OFL-*.txt "$S/fonts/"
cp LICENSE ACKNOWLEDGEMENTS.md "$S/"
sed -i 's/\r$//' "$S"/*.sh "$S"/*.txt "$S"/*.json "$S"/*.md "$S"/oracle.env.example
chmod 755 "$S/sage" "$S"/*.sh

ssh "$HOST" "mkdir -p $APP"
scp -q -O -r "$S"/* "$HOST:$APP/"
ssh "$HOST" "cd $APP && rm -f riddle \
  && { [ -f libquill.so ] || cp ../riddle/libquill.so .; } \
  && { [ -f libquillcompat.so ] || cp ../riddle/libquillcompat.so .; } \
  && { [ -f oracle.env ] && sed -i 's/^RIDDLE_/SAGE_/' oracle.env || { sed 's/^RIDDLE_/SAGE_/' ../riddle/oracle.env > oracle.env 2>/dev/null || cp oracle.env.example oracle.env; }; } \
  && chmod 600 oracle.env && ls -la"
if [ "$PUSH_CANON" = 1 ]; then
    C=$(mktemp -d)
    sed 's/\r$//' app/canon.txt > "$C/canon.txt"
    scp -q -O "$C/canon.txt" "$HOST:$APP/canon.txt"
    rm -rf "$C"
    echo "canon.txt copied to $HOST:$APP (personal use only)"
fi

# The English tile: a launcher only, reusing everything above.
E=$(mktemp -d)
cp app/en/external.manifest.json app/en/appload-launch.sh app/icon.png "$E/"
sed -i 's/\r$//' "$E"/*.sh "$E"/*.json
chmod 755 "$E/appload-launch.sh"
ssh "$HOST" "mkdir -p $APP-en"
scp -q -O "$E"/* "$HOST:$APP-en/"
rm -rf "$E"
echo "deployed to $HOST:$APP (贤者笔记) and $HOST:$APP-en (The Wise's Notes)"

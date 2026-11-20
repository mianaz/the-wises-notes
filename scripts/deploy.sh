#!/usr/bin/env bash
# Install or update both AppLoad tiles on the tablet over ssh.
#   ./scripts/deploy.sh            # after ./scripts/build.sh
#   ./scripts/deploy.sh --canon    # also copy app/canon.txt to the tablet
#
# It copies exactly what the release zip contains (scripts/make-bundle.sh), so
# a deployed notebook needs nothing else on the tablet: no riddle, no SDK.
# The device keeps its own oracle.env (API key) and canon.txt; this script
# never overwrites them unless --canon asks it to copy your local canon.txt
# (HoYoverse's script text, for your own personal use only: it goes to your
# own tablet and nowhere else). On first install it creates oracle.env from
# riddle's if you have one (renaming RIDDLE_* keys to SAGE_*), else from
# oracle.env.example.
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

./scripts/make-bundle.sh --stage
B=dist/bundle

ssh "$HOST" "mkdir -p $APP $APP-en"
scp -q -O -r "$B/sage-notes"/* "$HOST:$APP/"
scp -q -O -r "$B/sage-notes-en"/* "$HOST:$APP-en/"
ssh "$HOST" "cd $APP && rm -f riddle \
  && { [ -f oracle.env ] && sed -i 's/^RIDDLE_/SAGE_/' oracle.env \
       || { [ -f ../riddle/oracle.env ] && sed 's/^RIDDLE_/SAGE_/' ../riddle/oracle.env > oracle.env; } \
       || cp oracle.env.example oracle.env; } \
  && chmod 600 oracle.env && ls -la"

if [ "$PUSH_CANON" = 1 ]; then
    C=$(mktemp -d)
    sed 's/\r$//' app/canon.txt > "$C/canon.txt"
    scp -q -O "$C/canon.txt" "$HOST:$APP/canon.txt"
    rm -rf "$C"
    echo "canon.txt copied to $HOST:$APP (personal use only)"
fi
echo "deployed to $HOST:$APP (贤者笔记) and $HOST:$APP-en (The Wise's Notes)"

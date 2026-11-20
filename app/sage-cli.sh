#!/bin/bash
# Run the notebook's command-line checks over SSH with the same settings the
# notebook itself uses (persona, canon, fonts, oracle.env), without touching
# the screen or stopping xochitl:
#
#   ./sage-cli.sh --oracle-test          one test page to your model
#   ./sage-cli.sh --canon-test 当心，要下雨了   what a phrase would quote
#   ./sage-cli.sh --version
HERE=$(cd "$(dirname "$0")" && pwd)
export SAGE_PERSONA_FILE="$HERE/persona.txt"
export SAGE_CANON_FILE="$HERE/canon.txt"
export SAGE_FONT="$HERE/fonts/CrimsonPro-Italic.ttf"
export SAGE_FONT_CJK="$HERE/fonts/LXGWWenKai-Regular.ttf"
export SAGE_UI_LANG="${SAGE_UI_LANG:-zh}"
export SAGE_MEMORY_DIR=/home/root/sage-data/memories
if [ -f "$HERE/oracle.env" ]; then
    set -a; . "$HERE/oracle.env"; set +a
fi
cd "$HERE"
LD_LIBRARY_PATH="$HERE:/home/root/quill:/usr/lib/plugins/scenegraph" \
    HOME=/home/root exec "$HERE/sage" "$@"

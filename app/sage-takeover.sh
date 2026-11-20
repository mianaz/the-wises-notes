#!/bin/bash
# 贤者笔记 in full-takeover mode: the riddle engine with Hroptatyr's persona,
# a Chinese hand and Chinese UI. Stops xochitl, ALWAYS restores it on exit.
#
# Leave: power button sleeps, 5-finger tap exits. Escape hatch if anything
# wedges: ssh rm 'systemctl reset-failed xochitl; systemctl start xochitl'

restore() {
    rm -f /tmp/epframebuffer.lock
    # OS 3.29 guard: xochitl's StartLimitBurst=4/600s + OnFailure=emergency.target
    systemctl reset-failed xochitl 2>/dev/null
    systemctl start xochitl
}
trap restore EXIT INT TERM

HERE=$(cd "$(dirname "$0")" && pwd)

# The notebook's own defaults; oracle.env (API key, model) may override any.
export SAGE_PERSONA_FILE="$HERE/persona.txt"
export SAGE_CANON_FILE="$HERE/canon.txt"          # optional: your exact lines
export SAGE_FONT="$HERE/fonts/CrimsonPro-Italic.ttf"
export SAGE_FONT_CJK="$HERE/fonts/LXGWWenKai-Regular.ttf"
export SAGE_UI_LANG="${SAGE_UI_LANG:-zh}"       # the English tile passes en
export SAGE_TITLE="${SAGE_TITLE:-贤者笔记}"
export SAGE_MEMORY_DIR=/home/root/sage-data/memories
mkdir -p "$SAGE_MEMORY_DIR"

if [ -f "$HERE/oracle.env" ]; then
    set -a; . "$HERE/oracle.env"; set +a
fi
echo "sage-takeover: api=${SAGE_API:-auto} model=${SAGE_MODEL:-${SAGE_OPENAI_MODEL:-default}}"

systemctl stop xochitl
rm -f /tmp/epframebuffer.lock      # stale EPD lock blocks the engine
sleep 1

cd "$HERE"
# OS 3.29: libquill targets the pre-3.29 swapBuffers ABI; the shim bridges it.
LD_PRELOAD="$HERE/libquillcompat.so" \
LD_LIBRARY_PATH="$HERE:/home/root/quill:/usr/lib/plugins/scenegraph" \
    PAPERTERM_SHELL= HOME=/home/root \
    "$HERE/sage"
echo "sage-takeover: notebook closed ($?), restoring xochitl"

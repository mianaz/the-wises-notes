#!/usr/bin/env bash
# Fetch LXGW WenKai (霞鹜文楷, SIL OFL 1.1), the Chinese hand. It is about
# 25 MB, so it is fetched rather than committed.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=fonts/LXGWWenKai-Regular.ttf
[ -f "$OUT" ] && { echo "have $OUT"; exit 0; }
if [ -f "$HOME/fonts-cjk/LXGWWenKai/LXGWWenKai-Regular.ttf" ]; then
    cp "$HOME/fonts-cjk/LXGWWenKai/LXGWWenKai-Regular.ttf" "$OUT"
else
    curl -fL --proto '=https' -o "$OUT" \
        https://github.com/lxgw/LxgwWenKai/releases/latest/download/LXGWWenKai-Regular.ttf
fi
echo "fetched $OUT ($(du -h "$OUT" | cut -f1))"

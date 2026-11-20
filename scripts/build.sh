#!/usr/bin/env bash
# Build sage for the reMarkable Paper Pro (aarch64, takeover mode) from WSL or
# Linux, using zig as the C compiler and linker: no reMarkable SDK needed.
#
# Prereqs (one time):
#   - rustup with the aarch64-unknown-linux-gnu target
#   - zig, reachable as `python -m ziglang` (pip install ziglang) or on PATH
#   - libquill.so + libqsgepaper.so in $SAGE_LINK_DIR (default ~/sage-link),
#     copied from your own tablet:  ./scripts/build.sh --pull-libs
#
# Usage: ./scripts/build.sh [--pull-libs] [--test]
set -euo pipefail
cd "$(dirname "$0")/.."

LINK_DIR=${SAGE_LINK_DIR:-$HOME/sage-link}
HOST=${SAGE_HOST:-rm}
TOOLS=$HOME/.cache/sage-zig
mkdir -p "$TOOLS"

if command -v zig >/dev/null; then ZIG=(zig); else ZIG=(${SAGE_PYTHON:-python3} -m ziglang); fi

# zig wrappers: drop flags rustc and cc-rs pass that zig cc rejects.
mk() { # name, extra zig args
    cat > "$TOOLS/$1" <<EOF
#!/usr/bin/env bash
args=()
for a in "\$@"; do case "\$a" in --target=*|-Wl,--fix-cortex-a53-843419|-fuse-ld=lld|-B*gcc-ld) ;; *) args+=("\$a");; esac; done
exec ${ZIG[*]} cc $2 "\${args[@]}"
EOF
    chmod +x "$TOOLS/$1"
}
mk cc-host ""
mk cc-arm "-target aarch64-linux-gnu.2.39"
printf '#!/usr/bin/env bash\nexec %s ar "$@"\n' "${ZIG[*]}" > "$TOOLS/ar"; chmod +x "$TOOLS/ar"

export CC="$TOOLS/cc-host" AR="$TOOLS/ar"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$TOOLS/cc-host"
export CC_aarch64_unknown_linux_gnu="$TOOLS/cc-arm" AR_aarch64_unknown_linux_gnu="$TOOLS/ar"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$TOOLS/cc-arm"
export SAGE_LINK_DIR="$LINK_DIR"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/.cache/sage-target}
CARGO=${CARGO:-$HOME/.cargo/bin/cargo}

for arg in "$@"; do
    case "$arg" in
        --pull-libs)
            mkdir -p "$LINK_DIR"
            scp -O "$HOST:/home/root/xovi/exthome/appload/sage-notes/libquill.so" "$LINK_DIR/" 2>/dev/null \
              || scp -O "$HOST:/home/root/xovi/exthome/appload/riddle/libquill.so" "$LINK_DIR/"
            scp -O "$HOST:/usr/lib/plugins/scenegraph/libqsgepaper.so" "$LINK_DIR/"
            ;;
        --test)
            "$CARGO" test --release
            ;;
    esac
done

"$CARGO" build --release --target aarch64-unknown-linux-gnu --features takeover
BIN="$CARGO_TARGET_DIR/aarch64-unknown-linux-gnu/release/sage"
mkdir -p dist && cp "$BIN" dist/sage
echo "built: dist/sage ($(du -h dist/sage | cut -f1))"

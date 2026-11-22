#!/usr/bin/env bash
# Build sage for the reMarkable Paper Pro (aarch64, takeover mode) from WSL or
# Linux, using zig as the C compiler and linker: no reMarkable SDK needed.
#
# Prereqs (one time):
#   - rustup with the aarch64-unknown-linux-gnu target
#   - zig, reachable as `python -m ziglang` (pip install ziglang) or on PATH
#   - libqsgepaper.so (reMarkable's engine) in $SAGE_LINK_DIR (default
#     ~/sage-link), copied from your own tablet:  ./scripts/build.sh --pull-libs
#   - libquill.so is fetched on demand by scripts/fetch-quill.sh
#
# Output in dist/: sage, libquill.so, libquillcompat.so. Then
# ./scripts/deploy.sh installs them, or ./scripts/make-bundle.sh zips a release.
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

# Keep the builder's home and checkout paths out of released binaries. The
# encoded form (0x1f between flags) survives spaces in those paths.
SEP=$'\x1f'
CARGO_HOME_DIR=${CARGO_HOME:-$HOME/.cargo}
RUSTUP_HOME_DIR=${RUSTUP_HOME:-$HOME/.rustup}
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$PWD=/sage-notes$SEP--remap-path-prefix=$CARGO_HOME_DIR=/cargo$SEP--remap-path-prefix=$RUSTUP_HOME_DIR=/rustup"

for arg in "$@"; do
    case "$arg" in
        --pull-libs)
            # reMarkable's own engine: link against your tablet's copy.
            mkdir -p "$LINK_DIR"
            scp -O "$HOST:/usr/lib/plugins/scenegraph/libqsgepaper.so" "$LINK_DIR/"
            ;;
        --test)
            "$CARGO" test --release
            ;;
    esac
done

[ -f "$LINK_DIR/libqsgepaper.so" ] \
    || { echo "no $LINK_DIR/libqsgepaper.so: run ./scripts/build.sh --pull-libs once (tablet on ssh as $HOST)" >&2; exit 1; }
./scripts/fetch-quill.sh            # libquill.so, quill's MIT adapter (no-op once fetched)

"$CARGO" build --release --target aarch64-unknown-linux-gnu --features takeover
BIN="$CARGO_TARGET_DIR/aarch64-unknown-linux-gnu/release/sage"
mkdir -p dist && cp "$BIN" dist/sage
cp "$LINK_DIR/libquill.so" dist/libquill.so

# OS 3.29 swapBuffers shim (compat/quillcompat.c), preloaded by sage-takeover.sh.
"$TOOLS/cc-arm" -shared -fPIC -O2 -g0 -s -ffile-prefix-map="$PWD"=. \
    -o dist/libquillcompat.so compat/quillcompat.c -ldl
echo "built: dist/sage ($(du -h dist/sage | cut -f1)), dist/libquill.so, dist/libquillcompat.so"

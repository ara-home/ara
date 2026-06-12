#!/bin/sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${ROOT}/.bin"
RELEASE="${1:-debug}"

echo "  BUNDLE  ara"
mkdir -p "$DEST"

# Build Rust (ara-sec)
echo "  BUILD   ara-sec (Rust)"
case "$RELEASE" in
    debug)
        (cd "${ROOT}" && cargo build --quiet 2>&1)
        cp "${ROOT}/target/debug/ara-sec" "${DEST}/ara-sec"
        ;;
    release)
        (cd "${ROOT}" && cargo build --release --quiet 2>&1)
        cp "${ROOT}/target/release/ara-sec" "${DEST}/ara-sec"
        ;;
esac

# Build Zig (ara)
echo "  BUILD   ara (Zig)"
case "$RELEASE" in
    debug)
        (cd "${ROOT}" && zig build 2>&1)
        ;;
    release)
        (cd "${ROOT}" && zig build -Doptimize=ReleaseSafe 2>&1)
        ;;
esac
cp "${ROOT}/zig-out/bin/ara" "${DEST}/ara"

# Strip debug symbols on release
if [ "$RELEASE" = "release" ]; then
    strip "${DEST}/ara" "${DEST}/ara-sec" 2>/dev/null || true
fi

echo ""
echo "  DONE    $(du -sh "${DEST}" | cut -f1)  ${DEST}/"
ls -lh "${DEST}/"

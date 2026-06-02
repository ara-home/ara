#!/bin/sh
set -eu

ARA_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="${ARA_DIR}/.zig-cache"
PASS=0
FAIL=0

info()  { printf "  \033[1;34m[*]\033[0m %s\n" "$*"; }
ok()    { printf "  \033[1;32m[ok]\033[0m %s\n" "$*"; PASS=$((PASS + 1)); }
fail()  { printf "  \033[1;31m[FAIL]\033[0m %s\n" "$*"; FAIL=$((FAIL + 1)); }

cleanup() {
    if [ -n "${TMPDIR:-}" ]; then
        rm -rf "$TMPDIR"
    fi
}
trap cleanup EXIT

# Build
info "building ara..."
(cd "$ARA_DIR" && zig build 2>/dev/null)
ok "build succeeded"

ARABIN="${ARA_DIR}/zig-out/bin/ara"

# ---- E2E: simple-app with local dep ----
info "e2e: install local dependency"
TMPDIR=$(mktemp -d /tmp/ara-e2e-XXXXXX)
mkdir -p "$TMPDIR/project" "$TMPDIR/lib-a"

cp "$ARA_DIR/tests/fixtures/lib-a/ara.toml" "$TMPDIR/lib-a/"
cp "$ARA_DIR/tests/fixtures/simple-app/ara.toml" "$TMPDIR/project/"

(cd "$TMPDIR/project" && "$ARABIN" install 2>&1) | grep -q "Installing dependencies for simple-app"
if [ $? -eq 0 ]; then
    ok "install prints project name"
else
    fail "install did not print project name"
fi

# Check node_modules exists
if [ -d "$TMPDIR/project/node_modules/lib-a" ]; then
    ok "node_modules/lib-a/ directory created"
else
    fail "node_modules/lib-a/ missing"
fi

# Check ara.lock exists
if [ -f "$TMPDIR/project/ara.lock" ]; then
    ok "ara.lock written"
else
    fail "ara.lock missing"
fi

# Check ara.lock contains lib-a
grep -q "lib-a" "$TMPDIR/project/ara.lock" 2>/dev/null \
    && ok "ara.lock contains lib-a" \
    || fail "ara.lock missing lib-a"

cleanup

# ---- Summary ----
echo ""
if [ "$FAIL" -eq 0 ]; then
    printf "  \033[1;32mAll %d tests passed\033[0m\n" "$PASS"
else
    printf "  \033[1;31m%d passed, %d failed\033[0m\n" "$PASS" "$FAIL"
fi
exit "$FAIL"

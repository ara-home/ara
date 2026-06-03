#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

LINT_OPTS="-D clippy::all -D clippy::pedantic -W clippy::nursery -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic"

echo "  LINT   ara (src-rs)"
(cd src-rs && cargo clippy -- $LINT_OPTS 2>&1) || {
    echo "  FAIL   ara (src-rs) clippy failed"
    exit 1
}

echo "  LINT   ara-sec"
(cd ara-sec && cargo clippy -- $LINT_OPTS 2>&1) || {
    echo "  FAIL   ara-sec clippy failed"
    exit 1
}

echo "  PASS   all lints"

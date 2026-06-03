#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

echo "  LINT   ara (src-rs)"
(cd src-rs && cargo clippy -- -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic 2>&1) || {
    echo "  FAIL   ara (src-rs) clippy failed"
    exit 1
}

echo "  LINT   ara-sec"
(cd ara-sec && cargo clippy -- -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic 2>&1) || {
    echo "  FAIL   ara-sec clippy failed"
    exit 1
}

echo "  PASS   all lints"

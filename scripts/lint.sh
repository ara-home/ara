#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

LINT_OPTS="-D clippy::all -D clippy::pedantic -W clippy::nursery -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic"

echo "  LINT   ara"
(cd src && cargo clippy -- $LINT_OPTS 2>&1) || {
    echo "  FAIL   ara clippy failed"
    exit 1
}

echo "  PASS   all lints"

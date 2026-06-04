.PHONY: all build bundle test test-e2e test-all lint audit deny ci clean install-hooks

all: build build-sec

build:
	@echo "  BUILD  ara"
	@cd src && cargo build --quiet 2>&1

bundle: build build-sec
	@scripts/bundle.sh

test:
	@echo "  TEST   ara"
	@cd src && cargo test --quiet 2>&1

test-e2e: build build-sec
	@echo "  TEST   e2e"
	@tests/run.sh

test-all: test test test-e2e
	@echo "  ALL    tests passed"

lint:
	@echo "  LINT   ara"
	@cd src && cargo clippy -- -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic 2>&1
	@cd src && cargo fmt --check 2>&1

audit:
	@echo "  AUDIT  ara"
	@command -v cargo-audit >/dev/null 2>&1 || { echo "  SKIP   cargo-audit not installed (install with: cargo install cargo-audit)"; exit 0; }
	@cd src && cargo audit 2>&1

deny:
	@echo "  DENY   ara"
	@command -v cargo-deny >/dev/null 2>&1 || { echo "  SKIP   cargo-deny not installed (install with: cargo install cargo-deny)"; exit 0; }
	@cargo deny --workspace check 2>&1

ci: lint audit deny test
	@echo "  CI     all checks passed"

clean:
	@rm -rf .bin target
	@cd src && cargo clean 2>/dev/null
	@echo "  CLEAN"

install-hooks:
	@git config core.hooksPath .githooks/
	@echo "  HOOKS  installed"

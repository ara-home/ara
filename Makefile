.PHONY: all build bundle test test-e2e test-all lint-clean install-hooks

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

clean:
	@rm -rf .bin target
	@cd src && cargo clean 2>/dev/null
	@echo "  CLEAN"

install-hooks:
	@git config core.hooksPath .githooks/
	@echo "  HOOKS  installed"

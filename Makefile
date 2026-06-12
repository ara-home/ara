.PHONY: all build install bundle test test-e2e test-fixtures test-all lint audit deny udeps ci clean install-hooks

all: build

build:
	@echo "  BUILD  ara"
	@cargo build --quiet 2>&1

install:
	@echo "  BUILD  ara (release)"
	@cargo build --quiet --release 2>&1
	@mkdir -p .bin
	@cp target/release/ara .bin/ara
	@echo "  INSTALL ara -> .bin/ara"
	@echo ""
	@echo "Add .bin to your PATH:"
	@echo "  export PATH=\"$$(pwd)/.bin:\$$PATH\""

bundle: build
	@scripts/bundle.sh

test:
	@echo "  TEST   ara"
	@cargo test --quiet 2>&1

test-e2e: build
	@echo "  TEST   e2e"
	@crates/ara-cli/tests/run.sh

test-fixtures:
	@echo "  TEST   fixtures"
	@cargo test -p ara-cli --test fixture_test --quiet 2>&1

test-all: test test-e2e test-fixtures
	@echo "  ALL    tests passed"

lint:
	@echo "  LINT   ara"
	@cargo clippy -- -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic -D clippy::cargo -A clippy::multiple_crate_versions 2>&1
	@cargo fmt --check 2>&1

lint-pedantic:
	@echo "  LINT   pedantic"
	@cargo clippy -- -W clippy::pedantic 2>&1

audit:
	@echo "  AUDIT  ara"
	@command -v cargo-audit >/dev/null 2>&1 || { echo "  SKIP   cargo-audit not installed (install with: cargo install cargo-audit)"; exit 0; }
	@cargo audit 2>&1

deny:
	@echo "  DENY   ara"
	@command -v cargo-deny >/dev/null 2>&1 || { echo "  SKIP   cargo-deny not installed (install with: cargo install cargo-deny)"; exit 0; }
	@cargo deny --workspace check 2>&1

udeps:
	@echo "  UDPES  ara"
	@command -v cargo-udeps >/dev/null 2>&1 || { echo "  SKIP   cargo-udeps not installed (install with: cargo install cargo-udeps)"; exit 0; }
	@cargo udeps 2>&1

ci: lint audit deny udeps test
	@echo "  CI     all checks passed"

clean:
	@rm -rf .bin target
	@cargo clean 2>/dev/null
	@echo "  CLEAN"

install-hooks:
	@git config core.hooksPath .githooks/
	@echo "  HOOKS  installed"

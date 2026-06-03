.PHONY: all build build-sec bundle test test-sec test-e2e clean install-hooks

all: build build-sec

build:
	@echo "  BUILD  ara"
	@zig build

build-sec:
	@echo "  BUILD  ara-sec"
	@cd ara-sec && cargo build --quiet 2>&1

bundle: build build-sec
	@scripts/bundle.sh

test:
	@echo "  TEST   unit (ara)"
	@zig test src/main.zig

test-sec:
	@echo "  TEST   ara-sec"
	@cd ara-sec && cargo test --quiet 2>&1

test-e2e: build build-sec
	@echo "  TEST   e2e"
	@tests/run.sh

test-all: test test-sec test-e2e
	@echo "  ALL    tests passed"

clean:
	@rm -rf zig-out .zig-cache .bin
	@cd ara-sec && cargo clean 2>/dev/null
	@echo "  CLEAN"

install-hooks:
	@git config core.hooksPath .githooks/
	@echo "  HOOKS  installed"

run:
	@zig build run -- $(ARGS)

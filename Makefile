.PHONY: all build test test-e2e clean install-hooks

all: build

build:
	@echo "  BUILD  ara"
	@zig build

test:
	@echo "  TEST   unit"
	@zig build test

test-e2e: build
	@echo "  TEST   e2e"
	@tests/run.sh

clean:
	@rm -rf zig-out .zig-cache
	@echo "  CLEAN"

install-hooks:
	@git config core.hooksPath .githooks/
	@echo "  HOOKS  installed"

run:
	@zig build run -- $(ARGS)

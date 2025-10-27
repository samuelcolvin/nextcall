.DEFAULT_GOAL := all

# using pip install cargo (via maturin via pip) doesn't get the tty handle
# so doesn't render color without some help
export CARGO_TERM_COLOR=$(shell (test -t 0 && echo "always") || echo "auto")

.PHONY: build  ## Build binary and app
build:
	./build.sh

.PHONY: format  ## Auto-format rust and python source files
format:
	cargo fmt

.PHONY: lint  ## Lint rust source files
lint:
	cargo fmt
	cargo clippy -- -D warnings
	cargo check

.PHONY: all  ## Run the standard set of checks performed in CI
all: format build-dev lint

.PHONY: help  ## Display this message
help:
	@grep -E \
		'^.PHONY: .*?## .*$$' $(MAKEFILE_LIST) | \
		sort | \
		awk 'BEGIN {FS = ".PHONY: |## "}; {printf "\033[36m%-19s\033[0m %s\n", $$2, $$3}'

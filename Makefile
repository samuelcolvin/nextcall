.DEFAULT_GOAL := all

# using pip install cargo (via maturin via pip) doesn't get the tty handle
# so doesn't render color without some help
export CARGO_TERM_COLOR=$(shell (test -t 0 && echo "always") || echo "auto")

# Where `make install` puts the app. Override with `make install PREFIX=~/Applications`.
PREFIX := /Applications

.PHONY: build  ## Build binary and app
build:
	./build.sh

.PHONY: install  ## Build and install Nextcall.app to $(PREFIX), replacing any old copy
install: build
	@killall nextcall 2>/dev/null || true
	@mkdir -p $(PREFIX)
	rm -rf $(PREFIX)/Nextcall.app
	cp -R Nextcall.app $(PREFIX)/Nextcall.app
	@echo "Installed $(PREFIX)/Nextcall.app — launch with: open $(PREFIX)/Nextcall.app"

.PHONY: run  ## Build and run the app in the foreground (logs to terminal, Ctrl-C to quit)
run:
	./run.sh

.PHONY: format  ## Auto-format rust and python source files
format:
	cargo fmt

.PHONY: lint  ## Lint rust source files
lint:
	cargo fmt
	cargo clippy -- -D warnings

.PHONY: all  ## Run the standard set of checks performed in CI
all: format build-dev lint

.PHONY: help  ## Display this message
help:
	@grep -E \
		'^.PHONY: .*?## .*$$' $(MAKEFILE_LIST) | \
		sort | \
		awk 'BEGIN {FS = ".PHONY: |## "}; {printf "\033[36m%-19s\033[0m %s\n", $$2, $$3}'

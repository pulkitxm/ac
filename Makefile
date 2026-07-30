-include Makefile.local

CARGO_HOME  ?= $(HOME)/.cargo
RUSTUP_HOME ?= $(HOME)/.rustup
export CARGO_HOME
export RUSTUP_HOME
export PATH := $(CARGO_HOME)/bin:$(PATH)

CARGO := $(CARGO_HOME)/bin/cargo

BIN_DIR  ?= $(HOME)/.local/bin
BIN_NAME ?= ac


.DEFAULT_GOAL := help
.PHONY: help build dev test lint fmt install completions clean e2e

help: ## Show this help
	@echo 'ac - Apple Container project runner (Rust)'
	@echo
	@echo 'Usage: make <target>'
	@echo
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z0-9_-]+:.*?## / {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo
	@echo 'Toolchain (exported by every target):'
	@echo '  CARGO_HOME  $(CARGO_HOME)'
	@echo '  RUSTUP_HOME $(RUSTUP_HOME)'
	@echo
	@echo 'Install location (override on the command line):'
	@echo '  BIN_DIR     $(BIN_DIR)'
	@echo '  BIN_NAME    $(BIN_NAME)'

build: ## Build the optimised release binary (target/release/ac)
	'$(CARGO)' build --release

dev: ## Fast unoptimised build, for iterating
	'$(CARGO)' build

test: ## Run the unit tests
	AC_HOME='$(CURDIR)' '$(CARGO)' test

lint: ## Run clippy over all targets, warnings are errors
	'$(CARGO)' clippy --all-targets -- -D warnings

fmt: ## Format the source in place
	'$(CARGO)' fmt

install: build ## Build, then link the binary into BIN_DIR
	@mkdir -p '$(BIN_DIR)'
	@ln -sf '$(CURDIR)/target/release/ac' '$(BIN_DIR)/$(BIN_NAME)'
	@echo 'linked $(BIN_DIR)/$(BIN_NAME) -> $(CURDIR)/target/release/ac'
	@echo
	@echo 'Add to ~/.zshrc if not already present:'
	@echo '  export PATH="$(BIN_DIR):$$PATH"'
	@echo '  source <(COMPLETE=zsh $(BIN_NAME))'
	@echo
	@echo 'For bash, in ~/.bashrc:'
	@echo '  source <(COMPLETE=bash $(BIN_NAME))'

completions: build ## Print the shell hook to source (dynamic, always in sync)
	@echo '# add to ~/.zshrc:'
	@echo 'source <(COMPLETE=zsh ac)'
	@echo
	@echo '# bash, in ~/.bashrc:'
	@echo 'source <(COMPLETE=bash ac)'


test-completions: build ## Verify shell completion for every command, flag and value
	@./tests/completions.sh

e2e: build ## Run the integration tests against real containers
	@./tests/e2e.sh

clean: ## Remove build artefacts
	'$(CARGO)' clean

# Makefile - the entry point for building and testing the Rust `ac`.
#
# The Rust toolchain for this machine lives on an external SSD rather than in
# ~/.cargo, so every target exports it. That is the whole reason this file
# exists: `cargo build` on its own will not find a compiler.
#
# `make` with no target prints the help below.

# ------------------------------------------------------------- toolchain ---

# Note the unquoted spaces: make keeps them, and `export` hands the value to
# the recipe's environment intact.
export CARGO_HOME  := /Volumes/Sandisk SSD/.toolchains/cargo
export RUSTUP_HOME := /Volumes/Sandisk SSD/.toolchains/rustup
export PATH        := $(CARGO_HOME)/bin:$(PATH)

# Spelled out in full rather than relying on PATH. macOS ships GNU Make 3.81,
# which execs a recipe line directly when it holds no shell metacharacters, and
# that direct exec searches the PATH make itself started with, not the one
# exported above. An absolute path sidesteps it. The quotes at every use site
# matter too: the toolchain path contains a space.
CARGO := $(CARGO_HOME)/bin/cargo

# Where `make install` puts the binary. Override either to install elsewhere or
# under another name, for example to keep the bash `ac` on PATH at the same
# time:  make install BIN_NAME=ac-rs
BIN_DIR  ?= $(HOME)/.local/bin
BIN_NAME ?= ac

# Generated shell completions land here rather than in completions/, which
# holds the hand written ones belonging to the bash implementation.
COMPLETION_DIR := completions/rust

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
	'$(CARGO)' test

lint: ## Run clippy over all targets, warnings are errors
	'$(CARGO)' clippy --all-targets -- -D warnings

fmt: ## Format the source in place
	'$(CARGO)' fmt

install: build completions ## Build, then link the binary into BIN_DIR
	@mkdir -p '$(BIN_DIR)'
	@ln -sf '$(CURDIR)/target/release/ac' '$(BIN_DIR)/$(BIN_NAME)'
	@echo 'linked $(BIN_DIR)/$(BIN_NAME) -> $(CURDIR)/target/release/ac'
	@echo
	@echo 'Add to ~/.zshrc if not already present:'
	@echo '  export PATH="$(BIN_DIR):$$PATH"'
	@echo '  fpath=("$(CURDIR)/$(COMPLETION_DIR)" $$fpath)'
	@echo '  autoload -Uz compinit && compinit'
	@echo
	@echo 'For bash, source the completion directly:'
	@echo '  source "$(CURDIR)/$(COMPLETION_DIR)/ac.bash"'

completions: build ## Generate zsh, bash and fish completions into $(COMPLETION_DIR)
	@mkdir -p '$(COMPLETION_DIR)'
	@./target/release/ac completions zsh  > '$(COMPLETION_DIR)/_ac'
	@./target/release/ac completions bash > '$(COMPLETION_DIR)/ac.bash'
	@./target/release/ac completions fish > '$(COMPLETION_DIR)/ac.fish'
	@echo 'wrote $(COMPLETION_DIR)/{_ac,ac.bash,ac.fish}'

e2e: build ## Run the integration tests against real containers
	@./tests/e2e.sh

clean: ## Remove build artefacts
	'$(CARGO)' clean

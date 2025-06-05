# Makefile for octocode
# Best practices for Rust development and CI/CD

# Default target
.DEFAULT_GOAL := help

# Variables
BINARY_NAME := octocode
VERSION := $(shell grep '^version =' Cargo.toml | cut -d '"' -f2)
RUST_VERSION := $(shell rustc --version)
TARGET_DIR := target
RELEASE_DIR := $(TARGET_DIR)/release
DEBUG_DIR := $(TARGET_DIR)/debug

# Compilation targets
TARGETS := x86_64-unknown-linux-musl \
					aarch64-unknown-linux-musl \
					x86_64-pc-windows-msvc \
					aarch64-pc-windows-msvc \
					x86_64-apple-darwin \
					aarch64-apple-darwin

# Colors for output
GREEN := \033[0;32m
YELLOW := \033[0;33m
RED := \033[0;31m
BLUE := \033[0;34m
NC := \033[0m # No Color

# Check if we're in a git repository
GIT_AVAILABLE := $(shell git status >/dev/null 2>&1 && echo "yes" || echo "no")

.PHONY: help
help: ## Show this help message
	@echo "$(BLUE)octocode v$(VERSION)$(NC)"
	@echo "$(BLUE)Rust version: $(RUST_VERSION)$(NC)"
	@echo ""
	@echo "$(YELLOW)Available targets:$(NC)"
	@awk 'BEGIN {FS = ":.*##"; printf ""} /^[a-zA-Z_-]+:.*?##/ { printf "  $(GREEN)%-20s$(NC) %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

.PHONY: install-deps
install-deps: ## Install development dependencies
	@echo "$(YELLOW)Installing development dependencies...$(NC)"
	rustup component add clippy rustfmt
	cargo install cargo-audit cargo-outdated cargo-machete cargo-nextest
	@echo "$(GREEN)Dependencies installed successfully$(NC)"

.PHONY: install-targets
install-targets: ## Install compilation targets
	@echo "$(YELLOW)Installing compilation targets...$(NC)"
	@for target in $(TARGETS); do \
		echo "Installing $$target..."; \
		rustup target add $$target; \
	done
	@echo "$(GREEN)Compilation targets installed$(NC)"

.PHONY: check
check: ## Run cargo check
	@echo "$(YELLOW)Running cargo check...$(NC)"
	cargo check --all-targets --all-features

.PHONY: build
build: ## Build the project in debug mode
	@echo "$(YELLOW)Building $(BINARY_NAME) in debug mode...$(NC)"
	cargo build

.PHONY: build-release
build-release: ## Build the project in release mode
	@echo "$(YELLOW)Building $(BINARY_NAME) in release mode...$(NC)"
	cargo build --release
	@echo "$(GREEN)Release binary built: $(RELEASE_DIR)/$(BINARY_NAME)$(NC)"

.PHONY: build-static
build-static: ## Build static binary for current platform
	@echo "$(YELLOW)Building static binary...$(NC)"
ifeq ($(shell uname),Darwin)
	# macOS doesn't support fully static linking, but we can optimize
	cargo build --release --target x86_64-apple-darwin
	cargo build --release --target aarch64-apple-darwin
else ifeq ($(shell uname),Linux)
	# Linux static build with musl - native compilation
	# Both x86_64 and ARM64 can be built natively with musl-tools
ifeq ($(shell uname -m),x86_64)
	cargo build --release --target x86_64-unknown-linux-musl
else
	cargo build --release --target aarch64-unknown-linux-musl
endif
else
	# Fallback to regular release build
	cargo build --release
endif
	@echo "$(GREEN)Static binary built successfully$(NC)"

.PHONY: build-all
build-all: install-targets ## Build for all supported platforms
	@echo "$(YELLOW)Building for all supported platforms...$(NC)"
	@for target in $(TARGETS); do \
		echo "Building for $$target..."; \
		cargo build --release --target $$target; \
		if [ $$? -eq 0 ]; then \
			echo "$(GREEN)✓ $$target built successfully$(NC)"; \
		else \
			echo "$(RED)✗ $$target build failed$(NC)"; \
		fi; \
	done

.PHONY: test
test: ## Run tests
	@echo "$(YELLOW)Running tests...$(NC)"
	cargo test

.PHONY: test-verbose
test-verbose: ## Run tests with verbose output
	@echo "$(YELLOW)Running tests with verbose output...$(NC)"
	cargo test -- --nocapture

.PHONY: test-nextest
test-nextest: ## Run tests with nextest (faster test runner)
	@echo "$(YELLOW)Running tests with nextest...$(NC)"
	cargo nextest run

.PHONY: lint
lint: ## Run clippy lints
	@echo "$(YELLOW)Running clippy lints...$(NC)"
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: lint-fix
lint-fix: ## Run clippy with automatic fixes
	@echo "$(YELLOW)Running clippy with automatic fixes...$(NC)"
	cargo clippy --all-targets --all-features --fix --allow-dirty -- -D warnings

.PHONY: format
format: ## Format code with rustfmt
	@echo "$(YELLOW)Formatting code...$(NC)"
	cargo fmt

.PHONY: format-check
format-check: ## Check code formatting
	@echo "$(YELLOW)Checking code formatting...$(NC)"
	cargo fmt -- --check

.PHONY: audit
audit: ## Run security audit
	@echo "$(YELLOW)Running security audit...$(NC)"
	cargo audit

.PHONY: outdated
outdated: ## Check for outdated dependencies
	@echo "$(YELLOW)Checking for outdated dependencies...$(NC)"
	cargo outdated

.PHONY: unused-deps
unused-deps: ## Check for unused dependencies
	@echo "$(YELLOW)Checking for unused dependencies...$(NC)"
	cargo machete

.PHONY: clean
clean: ## Clean build artifacts
	@echo "$(YELLOW)Cleaning build artifacts...$(NC)"
	cargo clean
	@echo "$(GREEN)Build artifacts cleaned$(NC)"

.PHONY: clean-target
clean-target: ## Clean only target directory
	@echo "$(YELLOW)Cleaning target directory...$(NC)"
	rm -rf $(TARGET_DIR)
	@echo "$(GREEN)Target directory cleaned$(NC)"

.PHONY: install
install: build-release ## Install the binary to ~/.cargo/bin
	@echo "$(YELLOW)Installing $(BINARY_NAME)...$(NC)"
	cargo install --path .
	@echo "$(GREEN)$(BINARY_NAME) installed successfully$(NC)"

.PHONY: uninstall
uninstall: ## Uninstall the binary
	@echo "$(YELLOW)Uninstalling $(BINARY_NAME)...$(NC)"
	cargo uninstall $(BINARY_NAME)
	@echo "$(GREEN)$(BINARY_NAME) uninstalled successfully$(NC)"

.PHONY: install-completions
install-completions: build-release ## Install shell completions
	@echo "$(YELLOW)Installing shell completions...$(NC)"
	@./scripts/install-completions.sh
	@echo "$(GREEN)Shell completions installed!$(NC)"

.PHONY: test-completions
test-completions: build-release ## Test shell completion generation
	@echo "$(YELLOW)Testing shell completion generation...$(NC)"
	@./scripts/test-completions.sh

.PHONY: run
run: ## Run the application in debug mode
	@echo "$(YELLOW)Running $(BINARY_NAME) in debug mode...$(NC)"
	cargo run

.PHONY: run-release
run-release: ## Run the application in release mode
	@echo "$(YELLOW)Running $(BINARY_NAME) in release mode...$(NC)"
	cargo run --release

.PHONY: size
size: build-release ## Show binary size
	@echo "$(YELLOW)Binary size information:$(NC)"
	@if [ -f "$(RELEASE_DIR)/$(BINARY_NAME)" ]; then \
		ls -lh $(RELEASE_DIR)/$(BINARY_NAME); \
		file $(RELEASE_DIR)/$(BINARY_NAME); \
	else \
		echo "$(RED)Release binary not found. Run 'make build-release' first.$(NC)"; \
	fi

.PHONY: bench
bench: ## Run benchmarks
	@echo "$(YELLOW)Running benchmarks...$(NC)"
	cargo bench

.PHONY: doc
doc: ## Generate documentation
	@echo "$(YELLOW)Generating documentation...$(NC)"
	cargo doc --no-deps --open

.PHONY: doc-private
doc-private: ## Generate documentation including private items
	@echo "$(YELLOW)Generating documentation (including private)...$(NC)"
	cargo doc --no-deps --document-private-items --open

.PHONY: release-dry
release-dry: ## Dry run of cargo release
	@echo "$(YELLOW)Dry run of cargo release...$(NC)"
	cargo publish --dry-run

.PHONY: release
release: test lint audit ## Publish to crates.io
	@echo "$(YELLOW)Publishing to crates.io...$(NC)"
	cargo publish

.PHONY: git-tag
git-tag: ## Create and push git tag for current version
ifeq ($(GIT_AVAILABLE),yes)
	@echo "$(YELLOW)Creating git tag v$(VERSION)...$(NC)"
	git tag -a v$(VERSION) -m "Release v$(VERSION)"
	git push origin v$(VERSION)
	@echo "$(GREEN)Git tag v$(VERSION) created and pushed$(NC)"
else
	@echo "$(RED)Not in a git repository$(NC)"
endif

.PHONY: ci
ci: format-check lint test audit ## Run all CI checks locally
	@echo "$(GREEN)All CI checks passed!$(NC)"

.PHONY: ci-quick
ci-quick: format-check lint test ## Run quick CI checks (no audit)
	@echo "$(GREEN)Quick CI checks passed!$(NC)"

.PHONY: setup
setup: install-deps install-targets ## Setup development environment
	@echo "$(GREEN)Development environment setup complete!$(NC)"

.PHONY: info
info: ## Show project information
	@echo "$(BLUE)Project Information:$(NC)"
	@echo "  Name: $(BINARY_NAME)"
	@echo "  Version: $(VERSION)"
	@echo "  Rust version: $(RUST_VERSION)"
	@echo "  Target directory: $(TARGET_DIR)"
	@echo "  Release directory: $(RELEASE_DIR)"
	@echo "  Supported targets: $(TARGETS)"
	@if [ -f "$(RELEASE_DIR)/$(BINARY_NAME)" ]; then \
		echo "  Release binary: $(GREEN)✓ Available$(NC)"; \
	else \
		echo "  Release binary: $(RED)✗ Not built$(NC)"; \
	fi

# Create target directories if they don't exist
$(TARGET_DIR):
	mkdir -p $(TARGET_DIR)

$(RELEASE_DIR): $(TARGET_DIR)
	mkdir -p $(RELEASE_DIR)

$(DEBUG_DIR): $(TARGET_DIR)
	mkdir -p $(DEBUG_DIR)

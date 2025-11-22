.PHONY: help build test run clean fmt lint doc install release check all

# Default target
help:
	@echo "OAS-Gen2 Development Commands"
	@echo "=============================="
	@echo "make build       - Build in debug mode"
	@echo "make test        - Run all tests"
	@echo "make run         - Run CLI with petstore example"
	@echo "make clean       - Clean build artifacts"
	@echo "make fmt         - Format code"
	@echo "make lint        - Run clippy"
	@echo "make doc         - Generate and open documentation"
	@echo "make install     - Build and install binary"
	@echo "make release     - Build optimized release binary"
	@echo "make check       - Run fmt, lint, and test"
	@echo "make all         - Run check and build"

# Build in debug mode
build:
	cargo build

# Run all tests
test:
	cargo test --verbose

# Run CLI with example
run:
	cargo run --bin oas-gen -- examples/petstore.json -t typescript -v -o test-generated/typescript

# Run CLI with unkey example
run-unkey:
	cargo run --bin oas-gen -- examples/unkey.yml -t typescript -v -o test-generated/unkey-typescript

# Run CLI with stripe example
run-stripe:
	cargo run --bin oas-gen -- examples/stripe.json -t typescript -v -o test-generated/stripe-typescript

run-openrouter:
	cargo run --bin oas-gen -- examples/openrouter.yml -t typescript -v -o test-generated/openrouter-typescript

run-all: run run-unkey run-stripe run-openrouter

# Clean build artifacts
clean:
	cargo clean
	rm -rf test-generated/

# Format code
fmt:
	cargo fmt

# Run clippy
lint:
	cargo clippy -- -D warnings

# Generate documentation
doc:
	cargo doc --open --no-deps

# Build and install
install:
	cargo install --path cli

# Build release binary
release:
	cargo build --release
	@echo "Binary at: target/release/oas-gen"

# Run all checks
check: fmt lint test

# Full build pipeline
all: check build

# Test specific crate
test-parser:
	cargo test -p parser

test-ast:
	cargo test -p ast

test-codegen:
	cargo test -p codegen

test-generate:
	cargo test -p generate

test-typescript:
	cargo test -p typescript

# Watch mode (requires cargo-watch)
watch:
	cargo watch -x check -x test

# Generate from petstore with different styles
demo-per-service:
	cargo run --bin oas-gen -- examples/petstore.json -t typescript --service-style per-service -o demo-per-service

demo-single-client:
	cargo run --bin oas-gen -- examples/petstore.json -t typescript --service-style single-client -o demo-single-client

demo-by-tag:
	cargo run --bin oas-gen -- examples/petstore.json -t typescript --service-style by-tag -o demo-by-tag

# Clean demo outputs
clean-demo:
	rm -rf demo-per-service demo-single-client demo-by-tag

# Verify generated TypeScript compiles
verify-typescript: run
	@if [ -d "petstore-typescript" ]; then \
		echo "Checking TypeScript compilation..."; \
		cd petstore-typescript && npm install && npx tsc --noEmit || echo "TypeScript check failed"; \
	else \
		echo "No generated TypeScript found"; \
	fi

# Update dependencies
update:
	cargo update

# Show dependency tree
tree:
	cargo tree

# Build timings
timings:
	cargo build --timings

# Profile build
profile:
	cargo build --release --timings

# Run with backtrace
debug-run:
	RUST_BACKTRACE=1 cargo run --bin oas-gen -- examples/petstore.json -t typescript -v

# Check for outdated dependencies
outdated:
	cargo outdated


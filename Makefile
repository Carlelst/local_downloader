# Makefile for Video Processor development

.PHONY: help setup check test format clean doc bench install run

# Default target
help:
	@echo "Video Processor Development Commands"
	@echo "==================================="
	@echo ""
	@echo "Setup and Installation:"
	@echo "  setup     - Set up development environment"
	@echo "  install   - Install the binary"
	@echo ""
	@echo "Development:"
	@echo "  check     - Run all code quality checks"
	@echo "  test      - Run all tests"
	@echo "  format    - Format code"
	@echo "  clean     - Clean build artifacts"
	@echo ""
	@echo "Documentation:"
	@echo "  doc       - Generate and open documentation"
	@echo ""
	@echo "Performance:"
	@echo "  bench     - Run benchmarks"
	@echo ""
	@echo "Execution:"
	@echo "  run       - Run the application with default config"

# Setup development environment
setup:
	@echo "🛠️  Setting up development environment..."
	@./scripts/dev-setup.sh

# Run comprehensive code quality checks
check:
	@echo "🔍 Running code quality checks..."
	@./scripts/check.sh

# Run all tests
test:
	@echo "🧪 Running comprehensive test suite..."
	@./scripts/test.sh

# Format code
format:
	@echo "🎨 Formatting code..."
	@./scripts/format.sh

# Clean build artifacts
clean:
	@echo "🧹 Cleaning build artifacts..."
	@cargo clean
	@rm -rf coverage/
	@rm -rf target/doc/

# Generate and open documentation
doc:
	@echo "📚 Generating documentation..."
	@cargo doc --open --no-deps

# Run benchmarks
bench:
	@echo "⚡ Running benchmarks..."
	@cargo bench

# Install the binary
install:
	@echo "📦 Installing video_processor..."
	@cargo install --path .

# Run the application
run:
	@echo "🚀 Running video processor..."
	@cargo run

# Development workflow targets
dev-watch:
	@echo "👀 Starting development watch mode..."
	@cargo watch -x check -x test

dev-quick:
	@echo "⚡ Quick development check..."
	@cargo check && cargo test --lib

# CI/CD targets
ci-check: format check test
	@echo "✅ All CI checks passed"

# Release targets
release-check:
	@echo "🔍 Running release checks..."
	@cargo check --release
	@cargo test --release
	@cargo clippy --release -- -D warnings

release-build:
	@echo "🏗️  Building release version..."
	@cargo build --release

# Coverage report
coverage:
	@echo "📊 Generating coverage report..."
	@cargo tarpaulin --out Html --output-dir coverage
	@echo "Coverage report generated in coverage/tarpaulin-report.html"

# Security audit
audit:
	@echo "🔒 Running security audit..."
	@cargo audit

# Update dependencies
update:
	@echo "📦 Updating dependencies..."
	@cargo update
	@cargo outdated
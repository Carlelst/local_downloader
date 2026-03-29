#!/bin/bash

# Development environment setup script for Video Processor project
# This script installs required tools and sets up the development environment

set -e

echo "🛠️  Video Processor - Development Environment Setup"
echo "=================================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    local color=$1
    local message=$2
    echo -e "${color}${message}${NC}"
}

# Function to check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Function to install rustup component if not present
install_component() {
    local component=$1
    local description=$2
    
    if rustup component list --installed | grep -q "$component"; then
        print_status $GREEN "✅ $description is already installed"
    else
        print_status $YELLOW "📦 Installing $description..."
        rustup component add "$component"
        print_status $GREEN "✅ $description installed successfully"
    fi
}

# Function to install cargo tool if not present
install_cargo_tool() {
    local tool=$1
    local description=$2
    local install_command=$3
    
    if command_exists "$tool"; then
        print_status $GREEN "✅ $description is already installed"
    else
        print_status $YELLOW "📦 Installing $description..."
        eval "$install_command"
        print_status $GREEN "✅ $description installed successfully"
    fi
}

print_status $BLUE "🔍 Checking development environment..."
echo ""

# Check if Rust is installed
if ! command_exists rustc; then
    print_status $RED "❌ Rust is not installed. Please install Rust first:"
    echo "   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

print_status $GREEN "✅ Rust is installed ($(rustc --version))"

# Check if Cargo is installed
if ! command_exists cargo; then
    print_status $RED "❌ Cargo is not installed. Please reinstall Rust."
    exit 1
fi

print_status $GREEN "✅ Cargo is installed ($(cargo --version))"

echo ""
print_status $BLUE "📦 Installing required Rust components..."
echo ""

# Install required rustup components
install_component "rustfmt" "Rust formatter (rustfmt)"
install_component "clippy" "Rust linter (clippy)"

echo ""
print_status $BLUE "🔧 Installing development tools..."
echo ""

# Install useful cargo tools
install_cargo_tool "cargo-watch" "Cargo watch (auto-rebuild on changes)" "cargo install cargo-watch"
install_cargo_tool "cargo-edit" "Cargo edit (add/remove dependencies)" "cargo install cargo-edit"
install_cargo_tool "cargo-outdated" "Cargo outdated (check dependency updates)" "cargo install cargo-outdated"
install_cargo_tool "cargo-audit" "Cargo audit (security vulnerability scanner)" "cargo install cargo-audit"
install_cargo_tool "cargo-tarpaulin" "Cargo tarpaulin (code coverage)" "cargo install cargo-tarpaulin"

echo ""
print_status $BLUE "🔨 Setting up project-specific tools..."
echo ""

# Make scripts executable
chmod +x scripts/*.sh
print_status $GREEN "✅ Made all scripts executable"

# Create .cargo/config.toml for project-specific settings
mkdir -p .cargo
cat > .cargo/config.toml << 'EOF'
# Project-specific Cargo configuration

[build]
# Use all available CPU cores for compilation
jobs = 0

[target.x86_64-unknown-linux-gnu]
# Link with system libraries for better performance
rustflags = ["-C", "link-arg=-fuse-ld=lld"]

[alias]
# Useful aliases for development
check-all = "check --all-targets --all-features"
test-all = "test --all-targets --all-features"
fmt-all = "fmt --all"
clippy-all = "clippy --all-targets --all-features -- -D warnings"

# Quick development commands
dev-check = "check --all-targets"
dev-test = "test --lib"
dev-fmt = "fmt"
dev-clippy = "clippy --all-targets -- -D warnings"
EOF

print_status $GREEN "✅ Created .cargo/config.toml with useful aliases"

# Create .gitignore additions for development
if [ ! -f .gitignore ]; then
    touch .gitignore
fi

# Add development-specific ignores if not already present
grep -q "# Development tools" .gitignore || cat >> .gitignore << 'EOF'

# Development tools
/coverage/
*.profraw
*.profdata
/target/criterion/
/target/doc/
.vscode/
.idea/
*.swp
*.swo
*~

# OS specific
.DS_Store
Thumbs.db
EOF

print_status $GREEN "✅ Updated .gitignore with development-specific entries"

echo ""
print_status $BLUE "🎯 Setting up pre-commit hooks..."

# Create a simple pre-commit hook
mkdir -p .git/hooks
cat > .git/hooks/pre-commit << 'EOF'
#!/bin/bash
# Pre-commit hook for Video Processor project

echo "🔍 Running pre-commit checks..."

# Run formatting check
if ! cargo fmt -- --check; then
    echo "❌ Code formatting check failed. Run 'cargo fmt' to fix."
    exit 1
fi

# Run clippy
if ! cargo clippy --all-targets -- -D warnings; then
    echo "❌ Clippy check failed. Fix the warnings above."
    exit 1
fi

# Run tests
if ! cargo test --lib; then
    echo "❌ Tests failed. Fix the failing tests."
    exit 1
fi

echo "✅ All pre-commit checks passed!"
EOF

chmod +x .git/hooks/pre-commit
print_status $GREEN "✅ Created pre-commit hook"

echo ""
print_status $BLUE "📚 Creating development documentation..."

# Create development guide
cat > DEVELOPMENT.md << 'EOF'
# Development Guide

## Quick Start

1. **Setup development environment:**
   ```bash
   ./scripts/dev-setup.sh
   ```

2. **Run all checks:**
   ```bash
   ./scripts/check.sh
   ```

3. **Run all tests:**
   ```bash
   ./scripts/test.sh
   ```

4. **Format code:**
   ```bash
   ./scripts/format.sh
   ```

## Development Workflow

### Daily Development

```bash
# Start development with auto-rebuild
cargo watch -x check -x test

# Format code before committing
cargo fmt

# Run comprehensive checks
./scripts/check.sh
```

### Useful Cargo Aliases

```bash
# Quick checks during development
cargo dev-check    # Fast compilation check
cargo dev-test     # Run unit tests only
cargo dev-fmt      # Format code
cargo dev-clippy   # Run linter

# Comprehensive checks
cargo check-all    # Check all targets and features
cargo test-all     # Run all tests
cargo clippy-all   # Run clippy on everything
```

### Testing

```bash
# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Run property-based tests only
cargo test property

# Generate coverage report
cargo tarpaulin --out Html
```

### Documentation

```bash
# Generate and open documentation
cargo doc --open

# Test documentation examples
cargo test --doc
```

### Performance

```bash
# Run benchmarks
cargo bench

# Profile with perf (Linux)
cargo build --release
perf record --call-graph=dwarf target/release/video_processor
perf report
```

## Project Structure

```
├── src/
│   ├── presentation/     # CLI and user interfaces
│   ├── application/      # Business logic coordination
│   ├── domain/          # Core business logic
│   ├── infrastructure/  # External integrations
│   └── di/             # Dependency injection
├── tests/              # Integration tests
├── examples/           # Usage examples
├── scripts/            # Development scripts
└── docs/              # Documentation
```

## Code Quality Standards

- **Formatting**: Use `rustfmt` with default settings
- **Linting**: All clippy warnings must be addressed
- **Testing**: Maintain >80% test coverage
- **Documentation**: All public APIs must be documented
- **Performance**: Benchmark critical paths

## Contributing

1. Create feature branch from `main`
2. Make changes with tests
3. Run `./scripts/check.sh` to verify quality
4. Submit pull request with description

## Troubleshooting

### Common Issues

1. **Compilation errors**: Run `cargo clean` and rebuild
2. **Test failures**: Check for race conditions in concurrent tests
3. **Clippy warnings**: Use `cargo clippy --fix` for auto-fixes
4. **Documentation errors**: Ensure all examples compile

### Performance Issues

1. **Slow compilation**: Use `cargo check` for faster feedback
2. **Slow tests**: Run `cargo test --release` for optimized tests
3. **Memory usage**: Profile with `valgrind` or `heaptrack`
EOF

print_status $GREEN "✅ Created DEVELOPMENT.md guide"

echo ""
print_status $GREEN "🎉 Development environment setup completed successfully!"
echo ""
print_status $BLUE "📋 Summary of installed tools:"
echo "   ✅ rustfmt (code formatting)"
echo "   ✅ clippy (linting)"
echo "   ✅ cargo-watch (auto-rebuild)"
echo "   ✅ cargo-edit (dependency management)"
echo "   ✅ cargo-outdated (dependency updates)"
echo "   ✅ cargo-audit (security scanning)"
echo "   ✅ cargo-tarpaulin (code coverage)"
echo ""
print_status $BLUE "🚀 Next steps:"
echo "   1. Run './scripts/check.sh' to verify everything works"
echo "   2. Run './scripts/test.sh' to run the test suite"
echo "   3. Read DEVELOPMENT.md for detailed development guide"
echo "   4. Start coding with 'cargo watch -x check -x test'"
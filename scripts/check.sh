#!/bin/bash

# Code quality check script for Video Processor project
# This script runs various code quality checks including clippy, tests, and documentation

set -e

echo "🔍 Video Processor - Code Quality Check Script"
echo "============================================="

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

# Function to run a check and report status
run_check() {
    local check_name=$1
    local command=$2
    
    print_status $BLUE "🔄 Running: $check_name"
    
    if eval $command; then
        print_status $GREEN "✅ $check_name passed"
        return 0
    else
        print_status $RED "❌ $check_name failed"
        return 1
    fi
}

# Initialize error counter
errors=0

# Check if required tools are installed
echo "🛠️  Checking required tools..."

if ! command -v cargo &> /dev/null; then
    print_status $RED "❌ cargo is not installed"
    exit 1
fi

if ! command -v rustc &> /dev/null; then
    print_status $RED "❌ rustc is not installed"
    exit 1
fi

# Check for clippy
if ! rustup component list --installed | grep -q clippy; then
    print_status $YELLOW "⚠️  clippy is not installed. Installing..."
    rustup component add clippy
fi

print_status $GREEN "✅ All required tools are available"

echo ""
echo "🚀 Starting code quality checks..."
echo ""

# 1. Code compilation check
if ! run_check "Code Compilation" "cargo check --all-targets"; then
    ((errors++))
fi

echo ""

# 2. Clippy linting
if ! run_check "Clippy Linting" "cargo clippy --all-targets -- -D warnings"; then
    ((errors++))
fi

echo ""

# 3. Code formatting check
if ! run_check "Code Formatting" "cargo fmt -- --check"; then
    ((errors++))
fi

echo ""

# 4. Unit tests
if ! run_check "Unit Tests" "cargo test --lib"; then
    ((errors++))
fi

echo ""

# 5. Integration tests
if ! run_check "Integration Tests" "cargo test --test '*'"; then
    ((errors++))
fi

echo ""

# 6. Documentation build
if ! run_check "Documentation Build" "cargo doc --no-deps --document-private-items"; then
    ((errors++))
fi

echo ""

# 7. Example compilation
if ! run_check "Example Compilation" "cargo check --examples"; then
    ((errors++))
fi

echo ""

# Summary
echo "📊 Check Summary"
echo "==============="

if [ $errors -eq 0 ]; then
    print_status $GREEN "🎉 All checks passed! Code quality is excellent."
    echo ""
    print_status $BLUE "📈 Additional recommendations:"
    echo "   - Run 'cargo bench' for performance benchmarks"
    echo "   - Run 'cargo audit' to check for security vulnerabilities"
    echo "   - Consider running 'cargo outdated' to check for dependency updates"
else
    print_status $RED "❌ $errors check(s) failed. Please fix the issues above."
    echo ""
    print_status $YELLOW "💡 Quick fixes:"
    echo "   - Run './scripts/format.sh' to fix formatting issues"
    echo "   - Run 'cargo clippy --fix' to auto-fix some clippy warnings"
    echo "   - Check the error messages above for specific issues"
    exit 1
fi
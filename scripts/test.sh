#!/bin/bash

# Comprehensive test script for Video Processor project
# This script runs all types of tests including unit, integration, and property-based tests

set -e

echo "🧪 Video Processor - Comprehensive Test Script"
echo "=============================================="

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

# Function to run tests with proper error handling
run_test_suite() {
    local test_name=$1
    local command=$2
    local description=$3
    
    print_status $BLUE "🔄 Running: $test_name"
    print_status $YELLOW "   $description"
    
    if eval $command; then
        print_status $GREEN "✅ $test_name completed successfully"
        return 0
    else
        print_status $RED "❌ $test_name failed"
        return 1
    fi
}

# Initialize counters
total_tests=0
passed_tests=0

echo ""
print_status $BLUE "🚀 Starting comprehensive test suite..."
echo ""

# 1. Unit Tests
((total_tests++))
if run_test_suite "Unit Tests" \
    "cargo test --lib --verbose" \
    "Testing individual modules and functions"; then
    ((passed_tests++))
fi

echo ""

# 2. Integration Tests
((total_tests++))
if run_test_suite "Integration Tests" \
    "cargo test --test '*' --verbose" \
    "Testing component interactions and workflows"; then
    ((passed_tests++))
fi

echo ""

# 3. Property-Based Tests
((total_tests++))
if run_test_suite "Property-Based Tests" \
    "cargo test property --verbose" \
    "Testing correctness properties with generated inputs"; then
    ((passed_tests++))
fi

echo ""

# 4. Documentation Tests
((total_tests++))
if run_test_suite "Documentation Tests" \
    "cargo test --doc --verbose" \
    "Testing code examples in documentation"; then
    ((passed_tests++))
fi

echo ""

# 5. Example Tests (if examples can be tested)
((total_tests++))
if run_test_suite "Example Compilation" \
    "cargo check --examples" \
    "Verifying that all examples compile correctly"; then
    ((passed_tests++))
fi

echo ""

# 6. Benchmark Tests (if available)
if [ -d "benches" ] || grep -q "criterion" Cargo.toml; then
    ((total_tests++))
    if run_test_suite "Benchmark Tests" \
        "cargo bench --no-run" \
        "Compiling performance benchmarks"; then
        ((passed_tests++))
    fi
    echo ""
fi

# Test Coverage Report (if tarpaulin is available)
if command -v cargo-tarpaulin &> /dev/null; then
    print_status $BLUE "📊 Generating test coverage report..."
    if cargo tarpaulin --out Html --output-dir coverage; then
        print_status $GREEN "✅ Coverage report generated in coverage/tarpaulin-report.html"
    else
        print_status $YELLOW "⚠️  Coverage report generation failed"
    fi
    echo ""
fi

# Summary
echo "📊 Test Results Summary"
echo "======================"
print_status $BLUE "Total test suites: $total_tests"
print_status $BLUE "Passed: $passed_tests"
print_status $BLUE "Failed: $((total_tests - passed_tests))"

if [ $passed_tests -eq $total_tests ]; then
    print_status $GREEN "🎉 All test suites passed! Your code is working correctly."
    echo ""
    print_status $BLUE "📈 Next steps:"
    echo "   - Run './scripts/check.sh' for code quality analysis"
    echo "   - Run 'cargo bench' for detailed performance benchmarks"
    echo "   - Consider adding more property-based tests for edge cases"
else
    print_status $RED "❌ $((total_tests - passed_tests)) test suite(s) failed."
    echo ""
    print_status $YELLOW "💡 Debugging tips:"
    echo "   - Check the error messages above for specific failures"
    echo "   - Run individual test suites with 'cargo test <test_name>' for details"
    echo "   - Use 'cargo test -- --nocapture' to see println! output in tests"
    exit 1
fi

# Performance hint
echo ""
print_status $BLUE "⚡ Performance tip:"
echo "   Use 'cargo test --release' for faster test execution in CI/CD"
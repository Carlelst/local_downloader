#!/bin/bash

# Code formatting script for Video Processor project
# This script formats all Rust code using rustfmt and runs basic checks

set -e

echo "🎨 Video Processor - Code Formatting Script"
echo "=========================================="

# Check if rustfmt is installed
if ! command -v rustfmt &> /dev/null; then
    echo "❌ rustfmt is not installed. Please install it with:"
    echo "   rustup component add rustfmt"
    exit 1
fi

# Format all Rust files
echo "📝 Formatting Rust code..."
cargo fmt

# Check if there are any formatting issues
echo "🔍 Checking code formatting..."
if cargo fmt -- --check; then
    echo "✅ All code is properly formatted"
else
    echo "⚠️  Some files need formatting. Run 'cargo fmt' to fix them."
    exit 1
fi

echo "🎉 Code formatting completed successfully!"
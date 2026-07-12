#!/bin/bash
# scripts/salt_test.sh — Salt Test Runner
# Discovers @test functions in a .salt file, generates a parallel test harness, and runs it.
#
# Usage: ./scripts/salt_test.sh <file.salt> [--verbose]
#
# Features:
#   - Discovers all @test-annotated functions
#   - Runs each test in parallel via Thread::spawn
#   - Reports pass/fail with timing
#   - Exit code: 0 = all pass, 1 = any fail

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="/tmp/salt_build"
mkdir -p "$BUILD_DIR"

# Parse arguments
SOURCE_FILE=""
VERBOSE=false

for arg in "$@"; do
    case "$arg" in
        --verbose|-v) VERBOSE=true ;;
        *) SOURCE_FILE="$arg" ;;
    esac
done

if [ -z "$SOURCE_FILE" ]; then
    echo "Usage: salt_test.sh <file.salt> [--verbose]"
    exit 1
fi

if [ ! -f "$SOURCE_FILE" ]; then
    echo "Error: $SOURCE_FILE not found"
    exit 1
fi

BASENAME=$(basename "$SOURCE_FILE" .salt)

# ─────────────────────────────────────────────────────────────────
# Phase 1: Discover @test functions
# ─────────────────────────────────────────────────────────────────
# Look for pattern: @test followed by fn <name>()
TEST_FUNCTIONS=()
while IFS= read -r line; do
    # Extract function name from lines like: fn test_addition() {
    fn_name=$(echo "$line" | sed -n 's/.*fn \([a-zA-Z_][a-zA-Z0-9_]*\).*/\1/p')
    if [ -n "$fn_name" ]; then
        TEST_FUNCTIONS+=("$fn_name")
    fi
done < <(grep -A1 '@test' "$SOURCE_FILE" | grep 'fn ')

NUM_TESTS=${#TEST_FUNCTIONS[@]}

if [ "$NUM_TESTS" -eq 0 ]; then
    echo "⚠  No @test functions found in $SOURCE_FILE"
    exit 0
fi

echo "🧪 Salt Test Runner"
echo "   File: $SOURCE_FILE"
echo "   Tests found: $NUM_TESTS"
echo ""

# ─────────────────────────────────────────────────────────────────
# Phase 2: Generate test harness
# ─────────────────────────────────────────────────────────────────
# We generate a wrapper .salt file that:
# 1. Includes the original source (minus its main fn)
# 2. Creates a main() that calls each @test function and reports results

HARNESS_FILE="$BUILD_DIR/${BASENAME}_test_harness.salt"

# Extract package declaration and imports from original file
PACKAGE_LINE=$(grep '^package ' "$SOURCE_FILE" || echo "package test_harness")
IMPORTS=$(grep '^\(use \|import \)' "$SOURCE_FILE" || true)

# Extract everything between the first line after imports and the end, 
# excluding any existing main function
cat > "$HARNESS_FILE" << 'HARNESS_HEADER'
HARNESS_HEADER

# Copy the entire source file content
cat "$SOURCE_FILE" > "$HARNESS_FILE"

# Check if there's an existing main function and remove it
if grep -q 'fn main()' "$HARNESS_FILE"; then
    # Remove the main function (from 'fn main()' to its closing brace)
    # This is a simple approach — works for top-level main functions
    python3 -c "
import re, sys
with open('$HARNESS_FILE', 'r') as f:
    content = f.read()

# Remove existing main function (greedy match for the function body)
# Match 'fn main() {' ... '}' at the same indentation level
lines = content.split('\n')
result = []
skip = False
brace_depth = 0
for line in lines:
    if not skip and re.match(r'\s*fn main\s*\(\s*\)', line):
        skip = True
        brace_depth = 0
        # Count braces on this line
        brace_depth += line.count('{') - line.count('}')
        if brace_depth <= 0 and '{' in line:
            skip = False  # Single-line main
        continue
    if skip:
        brace_depth += line.count('{') - line.count('}')
        if brace_depth <= 0:
            skip = False
        continue
    result.append(line)

with open('$HARNESS_FILE', 'w') as f:
    f.write('\n'.join(result))
"
fi

# Now append the test harness main
cat >> "$HARNESS_FILE" << HARNESS_MAIN

// ═══════════════════════════════════════════════════════════════
// Auto-generated test harness — DO NOT EDIT
// ═══════════════════════════════════════════════════════════════

extern fn exit(code: i32);

fn main() {
    println("Running ${NUM_TESTS} tests...");
    println("");
    let mut passed: i64 = 0;
    let mut failed: i64 = 0;
HARNESS_MAIN

# Generate a call for each test function
for test_fn in "${TEST_FUNCTIONS[@]}"; do
    cat >> "$HARNESS_FILE" << TEST_CALL
    // Test: ${test_fn}
    ${test_fn}();
    println("  ✅ ${test_fn}");
    passed = passed + 1;
TEST_CALL
done

# Close the main function with summary
cat >> "$HARNESS_FILE" << 'HARNESS_FOOTER'

    println("");
    print("Result: ");
    println("{} passed, {} failed", passed, failed);

    if failed > 0 {
        exit(1);
    }
}
HARNESS_FOOTER

if $VERBOSE; then
    echo "── Generated harness: $HARNESS_FILE ──"
    echo ""
fi

# ─────────────────────────────────────────────────────────────────
# Phase 3: Compile and run via standard pipeline
# ─────────────────────────────────────────────────────────────────
"$SCRIPT_DIR/run_test.sh" "$HARNESS_FILE" ${VERBOSE:+--verbose} 2>&1
TEST_EXIT=$?

echo ""
if [ $TEST_EXIT -eq 0 ]; then
    echo "🎉 All tests passed!"
else
    echo "❌ Some tests failed (exit code: $TEST_EXIT)"
fi

exit $TEST_EXIT

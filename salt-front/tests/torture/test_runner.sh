#!/bin/bash
# Remove set -e to allow continuation after failures

# Configuration
TEST_DIR="salt-front/tests/torture/files"
SALT_BIN="cargo run --quiet --manifest-path salt-front/Cargo.toml --bin salt-front --"

echo "========================================"
echo "    DEFENSIVE SATURATION RUNNER"
echo "========================================"

FAILED_TESTS=0
TOTAL_TESTS=0

run_test() {
    local file=$1
    local filename=$(basename "$file")
    local should_fail=0
    
    if [[ "$filename" == fail_* ]]; then
        should_fail=1
    fi

    echo -n "Running $filename... "
    ((TOTAL_TESTS++))

    # Capture output and exit code
    OUTPUT=$($SALT_BIN --emit-mlir "$file" 2>&1)
    EXIT_CODE=$?
    
    # Check for error text in output (compiler may exit 0 even on error)
    HAS_ERROR=$(echo "$OUTPUT" | grep -i "^Error:" | head -1)

    if [ $should_fail -eq 1 ]; then
        if [ $EXIT_CODE -ne 0 ] || [ -n "$HAS_ERROR" ]; then
            echo "PASS (Expected Failure)"
            echo "  -> $HAS_ERROR"
        else
            echo "FAIL (Unexpected Success)"
            echo "Output:"
            echo "$OUTPUT" | head -5
            ((FAILED_TESTS++))
        fi
    else
        if [ $EXIT_CODE -eq 0 ] && [ -z "$HAS_ERROR" ]; then
            echo "PASS"
        else
            echo "FAIL (Unexpected Failure)"
            echo "Output:"
            echo "$OUTPUT" | head -5
            ((FAILED_TESTS++))
        fi
    fi
}

echo "--- Torture Tests ---"
for file in "$TEST_DIR"/*.salt; do
    run_test "$file"
done

echo ""
echo "--- CLI Saturation ---"
# Saturation loop for CLI flags
FLAGS=(
    "--version"
    "--help"
    "--emit-mlir"
    "--optimize"
    "--invalid-flag"
    "-O"
    "--unknown"
)

# Pick a valid file for these tests
VALID_FILE="$TEST_DIR/empty_loop.salt"

for flag in "${FLAGS[@]}"; do
    echo -n "Testing flags: '$flag'... "
    
    $SALT_BIN $flag "$VALID_FILE" > /dev/null 2>&1
    EXIT_CODE=$?
    
    # 101 is standard Rust panic exit code
    if [ $EXIT_CODE -eq 101 ]; then
        echo "FAIL (PANIC DETECTED)"
        ((FAILED_TESTS++))
    else
        echo "PASS (Exit Code: $EXIT_CODE)"
    fi
done

echo ""
echo "========================================"
if [ $FAILED_TESTS -eq 0 ]; then
    echo "SUMMARY: ALL TESTS PASSED ($TOTAL_TESTS tests)"
    exit 0
else
    echo "SUMMARY: $FAILED_TESTS FAILURES DETECTED"
    exit 1
fi

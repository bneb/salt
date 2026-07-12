#!/bin/bash
set -e

echo "=== SATURATION ATTACK STARTED ==="

# 1. Rust Coverage
echo "[1/3] Running Rust Frontend Tests..."
if command -v cargo-llvm-cov &> /dev/null; then
    cargo llvm-cov --html --output-dir target/llvm-cov
else
    echo "cargo-llvm-cov not found, falling back to cargo test..."
    cargo test
fi

# 2. Coverage Torture Compilation
echo "[2/3] Compiling Coverage Torture Suite..."
cargo run --bin salt-front -- tests/cases/coverage_torture.salt -o /tmp/torture.mlir --skip-scan

# 3. Z3 Logic Verification
echo "[3/3] Running Z3 Verification Pathology..."
# First compile to MLIR
cargo run --bin salt-front -- tests/cases/verification_pathology.salt -o tests/cases/verification_pathology.mlir --skip-scan

# Then run salt-opt with verification (assuming salt-opt is in the path or typical build location)
SALT_OPT=../salt/build/bin/salt-opt
if [ -f "$SALT_OPT" ]; then
    echo "Running salt-opt on verification_pathology.mlir..."
    # We expect failures/timeouts here, so allow failure
    $SALT_OPT --verify < tests/cases/verification_pathology.mlir || true
else
    echo "salt-opt not found at $SALT_OPT. Skipping backend verification."
fi

echo "=== SATURATION ATTACK COMPLETE ==="

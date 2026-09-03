#!/usr/bin/env bash
# Fuzz-campaign runner (S8 maturity ratchet).
#
# Drives the bounded campaign defined in
# salt-front/tests/fuzz_campaign_test.rs: deterministic byte streams ->
# fuzz_ast::FuzzSaltFile -> to_salt() -> saltc::compile_ast, asserting that
# no generated program panics or raises an [E007] internal compiler error.
# Per-iteration pass/reject/skip/crash stats plus a delta against the previous
# run are written to .round1-staging/fuzz-results/ by the test itself.
#
# Usage:
#   bash scripts/fuzz_campaign.sh [--iterations N] [--timeout-secs T]
#
# Defaults: 100 iterations, 10s overall wall-clock budget (enforced inside
# the test as a stop-starting-new-iterations deadline; a single iteration is
# not preemptible from within the process).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ITERATIONS="${FUZZ_ITERATIONS:-100}"
TIMEOUT_SECS="${FUZZ_TIMEOUT_SECS:-10}"
export LIBRARY_PATH="${LIBRARY_PATH:-}:/opt/homebrew/lib:/usr/local/lib"

while [ $# -gt 0 ]; do
  case "$1" in
    --iterations)   ITERATIONS="$2"; shift 2;;
    --timeout-secs) TIMEOUT_SECS="$2"; shift 2;;
    *) echo "unknown argument: $1 (usage: [--iterations N] [--timeout-secs T])" >&2; exit 2;;
  esac
done

RESULTS_DIR="$ROOT/.round1-staging/fuzz-results"
mkdir -p "$RESULTS_DIR"

echo "fuzz-campaign: $ITERATIONS iterations, ${TIMEOUT_SECS}s budget, results -> $RESULTS_DIR"
export FUZZ_ITERATIONS="$ITERATIONS" FUZZ_TIMEOUT_SECS="$TIMEOUT_SECS"

# The integration test compiles against the library and exercises the same
# pipeline as the CLI binary; --nocapture streams its progress line.
(cd "$ROOT/salt-front" && cargo test --release --test fuzz_campaign_test -- --nocapture)

if [ -f "$RESULTS_DIR/latest-summary.json" ]; then
  echo "--- latest campaign summary ---"
  cat "$RESULTS_DIR/latest-summary.json"
fi

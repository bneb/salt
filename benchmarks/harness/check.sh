#!/bin/bash
# Verify every Salt benchmark compiles and its Z3 contracts pass.
# CI-safe: needs only saltc.
#
# Fixed fork of salt-benchmarks/harness/check.sh. Differences from upstream:
#   * PROBLEMS_DIR / SALTC parameters with workspace-local defaults.
#   * FAIL rows show the first compiler error lines instead of swallowing stderr,
#     so a failing contract is diagnosable straight from the run output.
#   * No bare set -e: counters and reporting stay in our control; failures are
#     explicit, visible, and reflected in the summary plus exit code.
set -u -o pipefail

HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_ROOT="$(cd "$HARNESS_DIR/../.." && pwd)"
PROBLEMS_DIR="${PROBLEMS_DIR:-$WS_ROOT/benchmarks/problems}"
SALTC="${SALTC:-$WS_ROOT/.round1-staging/saltc-snapshot}"
LOG_DIR="${LOG_DIR:-$WS_ROOT/.round1-staging/build}"

PASS=0; FAIL=0; SKIP=0

[ -x "$SALTC" ] || { echo "saltc not found or not executable: $SALTC" >&2; exit 1; }
[ -d "$PROBLEMS_DIR" ] || { echo "PROBLEMS_DIR not found: $PROBLEMS_DIR" >&2; exit 1; }
mkdir -p "$LOG_DIR" 2>/dev/null || { echo "cannot create LOG_DIR: $LOG_DIR" >&2; exit 1; }

resolve_src() {
    local f="$1/$(basename "$1").salt"
    if [ -f "$f" ]; then PICKED="$f"; return 0; fi
    f=$(printf '%s\n' "$1"/*.salt 2>/dev/null | sort | head -1)
    case "$f" in *.salt) [ -f "$f" ] && { PICKED="$f"; return 0; } ;; esac
    PICKED=""
}

check_one() {  # $1=problem directory
    local d="$1" name src log
    name=$(basename "$d")
    resolve_src "$d"; src=$PICKED
    if [ -z "$src" ]; then
        printf "  %-30s %s\n" "$name" "SKIP (no .salt file)"
        SKIP=$((SKIP + 1)); return 0
    fi
    log="$LOG_DIR/${name}.saltc.log"
    if "$SALTC" "$src" --lib --disable-alias-scopes -o /dev/null 2>"$log"; then
        printf "  %-30s %s\n" "$name" "PASS"
        PASS=$((PASS + 1))
    else
        printf "  %-30s %s\n" "$name" "FAIL"
        grep -m 3 -E '[^[:space:]]' "$log" | sed 's/^/      error: /' >&2
        FAIL=$((FAIL + 1))
    fi
}

echo "saltc: $("$SALTC" --version 2>/dev/null || echo unknown)"
echo "problems: $PROBLEMS_DIR"
echo ""
for d in "$PROBLEMS_DIR"/*/; do
    [ -d "$d" ] || continue
    check_one "$d"
done
echo ""
echo "$PASS passed, $FAIL failed, $SKIP skipped"
[ "$FAIL" -gt 0 ] && exit 1
exit 0

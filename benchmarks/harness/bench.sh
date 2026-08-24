#!/bin/bash
# Local Salt-benchmarks harness: compile + time C/Rust, verify Salt contracts.
#
# Fixed fork of salt-benchmarks/harness/bench.sh. Differences from upstream:
#   * No silent whole-run aborts. Upstream ran under set -euo pipefail with the
#     timing pipeline left unguarded; a benchmark binary exiting non-zero made
#     /usr/bin/time propagate that status, pipefail failed the pipeline, and
#     errexit killed the entire script without printing anything. Here every
#     failure prints a visible "[warn] ..." line and the run degrades gracefully.
#   * Per-execution watchdog. Upstream had no timeout, so a program that never
#     exits (problems/echo is an infinite TCP server) blocked the harness forever.
#   * True median-of-RUNS. Upstream summed samples and divided by RUNS (a mean),
#     although RESULTS.md described the number as a median. We report the actual
#     median of the valid samples.
#   * Parameterized PROBLEMS_DIR / BUILD_OUT / SALTC. Defaults stay inside this
#     workspace; BUILD_OUT outside it is refused.
set -u -o pipefail

HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_ROOT="$(cd "$HARNESS_DIR/../.." && pwd)"
PROBLEMS_DIR="${PROBLEMS_DIR:-$WS_ROOT/benchmarks/problems}"
BUILD_OUT="${BUILD_OUT:-$WS_ROOT/.round1-staging/build}"
SALTC="${SALTC:-$WS_ROOT/.round1-staging/saltc-snapshot}"
CC="${CC:-clang}"; RUSTC="${RUSTC:-rustc}"
RUNS="${RUNS:-5}"; WARMUP="${WARMUP:-2}"; TIMEOUT_SECS="${TIMEOUT_SECS:-10}"

GREEN='\033[32m'; YELLOW='\033[33m'; RED='\033[31m'; NC='\033[0m'
WARNS_FILE="$BUILD_OUT/.warnings"
SECS=""; RC=0

case "$BUILD_OUT" in "$WS_ROOT"/*) ;; *)
    echo "refusing BUILD_OUT outside $WS_ROOT: $BUILD_OUT" >&2; exit 1 ;;
esac
[ -d "$PROBLEMS_DIR" ] || { echo "PROBLEMS_DIR not found: $PROBLEMS_DIR" >&2; exit 1; }
mkdir -p "$BUILD_OUT" 2>/dev/null || { echo "cannot create BUILD_OUT: $BUILD_OUT" >&2; exit 1; }
: >"$WARNS_FILE" 2>/dev/null || true

# Warning count lives in a file because some callers run inside command
# substitutions; a shell variable would be incremented in a discarded subshell.
warn() { printf '[warn] %s: %s\n' "$1" "$2" >&2; echo x >>"$WARNS_FILE" 2>/dev/null || true; }

# guard_cmd CMD ARGS... -- run one command under a watchdog that kills the whole
# process group on expiry (macOS ships neither timeout nor gtimeout).
guard_cmd() {
    if command -v timeout >/dev/null 2>&1; then
        timeout "$TIMEOUT_SECS" "$@"
    elif command -v gtimeout >/dev/null 2>&1; then
        gtimeout "$TIMEOUT_SECS" "$@"
    else
        perl -e 'my $t = shift @ARGV; my $pid = fork();
            if ($pid == 0) { setpgrp(0, 0); exec @ARGV or exit 127; }
            $SIG{ALRM} = sub { kill "-9", $pid; waitpid $pid, 0; exit 124; };
            alarm $t; waitpid $pid, 0; exit $? >> 8;' \
            "$TIMEOUT_SECS" "$@"
    fi
}

# sample_seconds BINARY -- one timed execution; sets SECS (empty on failure) and RC.
sample_seconds() {
    local out t
    out=$(guard_cmd /usr/bin/time -p "$1" 2>&1 >/dev/null)
    RC=$?
    t=$(printf '%s\n' "$out" | awk '$1 == "real" {print $2}' | head -1)
    case "$t" in
        ''|*[!0-9.]*) SECS=""; return 1 ;;
        *) SECS="$t"; return 0 ;;
    esac
}

# median_of -- newline-separated samples on stdin, median printed to 4 decimals.
median_of() {
    sort -g | awk 'NF {v[++N] = $1}
        END {
            if (N == 0) exit 1;
            if (N % 2) printf "%.4f\n", v[(N + 1) / 2];
            else printf "%.4f\n", (v[N / 2] + v[N / 2 + 1]) / 2;
        }'
}

# resolve_src DIR .EXT -- PICKED = deterministic source path ("" when absent).
resolve_src() {
    local f="$1/$(basename "$1")$2"
    if [ -f "$f" ]; then PICKED="$f"; return 0; fi
    f=$(printf '%s\n' "$1"/*"$2" 2>/dev/null | sort | head -1)
    case "$f" in *"$2") [ -f "$f" ] && { PICKED="$f"; return 0; } ;; esac
    PICKED=""
}

# time_lang KIND SRC OUT LABEL -- compile, warm up, RUNS timed reps; print median.
time_lang() {
    local kind="$1" src="$2" out="$3"
    local label="$4"
    local log="$BUILD_OUT/${label}.${kind}"
    local samples="" i n=0
    case "$kind" in
        c)  "$CC" -O3 -o "$out" "$src" -lm 2>"${log}.log" ;;
        rs) "$RUSTC" -C opt-level=3 -o "$out" "$src" 2>"${log}.log" ;;
        *) return 1 ;;
    esac || { warn "$label" "$kind compile failed -- see ${log}.log"; return 1; }
    for ((i = 0; i < WARMUP; i++)); do guard_cmd "$out" >/dev/null 2>&1 || true; done
    for ((i = 0; i < RUNS; i++)); do
        sample_seconds "$out"
        if [ -n "$SECS" ]; then
            samples="${samples}${SECS}"$'\n'; n=$((n + 1))
            [ "$RC" -ne 0 ] && warn "$label" "$kind run $((i+1)) exited rc=$RC (sample kept)"
        else
            warn "$label" "$kind run $((i+1))/$RUNS gave no time (watchdog ${TIMEOUT_SECS}s or exec failure)"
        fi
    done
    if [ "$n" -eq 0 ]; then warn "$label" "no valid $kind samples"; return 1; fi
    [ "$n" -lt "$RUNS" ] && warn "$label" "median over $n/$RUNS valid $kind samples only"
    median_of <<<"$samples"
}

verify_salt() {  # $1=.salt source -> saltc contract-check status
    "$SALTC" "$1" --lib --disable-alias-scopes -o /dev/null 2>"$BUILD_OUT/saltc-last.log"
}

bench_one() {  # $1=problem directory
    local d="$1" name s c r t ok=0
    name=$(basename "$d")
    resolve_src "$d" ".c";   c=$PICKED
    resolve_src "$d" ".rs";  r=$PICKED
    resolve_src "$d" ".salt"; s=$PICKED
    printf "  %-26s" "$name"
    if [ -n "$c" ]; then
        t=$(time_lang c "$c" "$BUILD_OUT/${name}_c" "$name")
        if [ -n "$t" ]; then printf "  C=%ss" "$t"; ok=1; else printf "  ${YELLOW}C✗${NC}"; fi
    fi
    if [ -n "$r" ]; then
        t=$(time_lang rs "$r" "$BUILD_OUT/${name}_rs" "$name")
        if [ -n "$t" ]; then printf "  R=%ss" "$t"; ok=1; else printf "  ${YELLOW}R✗${NC}"; fi
    fi
    if [ -n "$s" ]; then
        if verify_salt "$s"; then printf "  ${GREEN}S✓${NC}"; ok=1
        else printf "  ${RED}S✗${NC}"
             warn "$name" "Salt contract verification failed -- see $BUILD_OUT/saltc-last.log"; fi
    fi
    echo ""
    [ "$ok" -eq 1 ] || warn "$name" "produced no results at all"
    return 0
}

main() {
    echo "salt-benchmarks (local harness)"
    echo "problems: $PROBLEMS_DIR"
    echo "cc:     $(command -v "$CC" 2>/dev/null || echo 'not found')"
    echo "rustc:  $(command -v "$RUSTC" 2>/dev/null || echo 'not found')"
    echo "saltc:  $SALTC"
    echo "runs:   $RUNS  (warmup: $WARMUP, per-run watchdog: ${TIMEOUT_SECS}s, median reported)"
    echo ""
    local arg d
    if [ $# -gt 0 ]; then
        for arg in "$@"; do
            d="$PROBLEMS_DIR/$arg"
            if [ -d "$d" ]; then bench_one "$d"; else warn "$arg" "no such problem directory"; fi
        done
    else
        for d in "$PROBLEMS_DIR"/*/; do [ -d "$d" ] && bench_one "$d"; done
    fi
    echo ""
    WARNS=$(wc -l < "$WARNS_FILE" 2>/dev/null | tr -d ' ')
    echo "done: ${WARNS:-0} warning(s); each figure is the median of $RUNS runs after $WARMUP warmups"
}

main "$@"
exit 0

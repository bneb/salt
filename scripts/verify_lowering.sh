#!/usr/bin/env bash
# =============================================================================
# verify_lowering.sh -- verification net for saltc MLIR emission (W2/lowering)
#
# Compiles representative Salt programs to textual MLIR with the release
# salt-front binary and asserts, per program:
#   (a) core structural markers of the emitted module,
#   (b) expected function symbols are present,
#   (c) byte-for-byte determinism across two compilations.
# Any failed assertion prints a clear message and makes the script exit 1.
#
# Usage:
#   scripts/verify_lowering.sh                 # run the full suite
#   scripts/verify_lowering.sh --corrupt-check FILE SYM[,SYM...] [REGEX]
#       # run the same assertions against an existing .mlir file
#       # (self-test hook used to prove the net detects corruption)
#
# Environment overrides:
#   SALTC   path to saltc binary (default: salt-front/target/release/saltc)
# =============================================================================
set -u -o pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SALTC="${SALTC:-$REPO_ROOT/salt-front/target/release/saltc}"

PASS_COUNT=0
FAIL_COUNT=0

fail_msg() {
    printf 'FAIL: %s\n' "$1"
    FAIL_COUNT=$((FAIL_COUNT + 1))
}

pass_msg() {
    printf 'PASS: %s\n' "$1"
    PASS_COUNT=$((PASS_COUNT + 1))
}

# check_structure FILE NAME -- core MLIR structural markers of saltc output.
check_structure() {
    local _f="$1"; _name="$2"
    if ! grep -q 'module attributes {' "$_f"; then
        fail_msg "$_name: no 'module attributes' header found"
        return 1
    fi
    if ! grep -q 'llvm.data_layout' "$_f"; then
        fail_msg "$_name: missing llvm.data_layout module attribute"
        return 1
    fi
    if ! grep -q 'llvm.target_triple' "$_f"; then
        fail_msg "$_name: missing llvm.target_triple module attribute"
        return 1
    fi
    if ! grep -Eq '(func|llvm)[.]func[^@]*@[A-Za-z_]' "$_f"; then
        fail_msg "$_name: no function definition found"
        return 1
    fi
    check_braces "$_f" "$_name" || return 1
    local _tail="$(tail -n 1 "$_f" | tr -d '[:space:]')"
    if [ "$_tail" != '}' ]; then
        fail_msg "$_name: module closing brace missing (file truncated?)"
        return 1
    fi
    return 0
}

# check_braces FILE NAME -- open/close brace counts must match and be nonzero.
# Catches tail-truncation at brace boundaries that a last-line check misses.
check_braces() {
    local _f="$1"; _name="$2"
    local _open=$(grep -o '{' "$_f" | wc -l | tr -d '[:space:]')
    local _close=$(grep -o '}' "$_f" | wc -l | tr -d '[:space:]')
    if [ "$_open" -eq 0 ] || [ "$_open" -ne "$_close" ]; then
        fail_msg "$_name: unbalanced braces (open=$_open close=$_close)"
        return 1
    fi
    return 0
}

# check_symbols FILE NAME SYM_CSV -- every comma-separated symbol must appear.
check_symbols() {
    local _f="$1"; _name="$2"; _syms="$3"
    local _oldifs=$IFS
    IFS=','
    for _sym in $_syms; do
        if ! grep -qE "@${_sym}([^A-Za-z0-9_]|$)" "$_f"; then
            IFS=$_oldifs
            fail_msg "$_name: expected symbol '@$_sym' not found"
            return 1
        fi
    done
    IFS=$_oldifs
    return 0
}

# check_marker FILE NAME REGEX -- extra structural regex (e.g. affine[.]for).
check_marker() {
    local _f="$1"; _name="$2"; _regex="$3"
    if ! grep -qE "$_regex" "$_f"; then
        fail_msg "$_name: expected pattern '$_regex' not found"
        return 1
    fi
    return 0
}

# compile_prog SRC OUT -- runs saltc quietly; stderr captured for diagnostics.
compile_prog() {
    local _src="$1"; _out="$2"
    "$SALTC" "$_src" -o "$_out" >/dev/null 2>"$ERR_LOG"
    return $?
}

# check_file NAME FILE SYMS MARKER -- all content assertions for one artifact.
check_file() {
    local _name="$1"; _file="$2"; _syms="$3"; _marker="$4"
    if ! check_structure "$_file" "$_name"; then return 1; fi
    if ! check_symbols "$_file" "$_name" "$_syms"; then return 1; fi
    if [ -n "$_marker" ]; then
        if ! check_marker "$_file" "$_name" "$_marker"; then return 1; fi
    fi
    return 0
}

# verify_case SPEC -- SPEC is "name|source|syms|optional_marker".
verify_case() {
    local _spec="$1"
    local _name=$(printf '%s' "$_spec" | cut -d'|' -f1)
    local _src=$(printf '%s' "$_spec" | cut -d'|' -f2)
    local _syms=$(printf '%s' "$_spec" | cut -d'|' -f3)
    local _marker=$(printf '%s' "$_spec" | cut -d'|' -f4)
    local _out_a="${WORK_DIR}/$_name.a.mlir"
    local _out_b="${WORK_DIR}/$_name.b.mlir"

    if ! compile_prog "$REPO_ROOT/$_src" "$_out_a"; then
        fail_msg "$_name: saltc exited nonzero ($_src); stderr tail:"
        sed 's/^/      /' "$ERR_LOG" | tail -n 3
        return
    fi
    if ! compile_prog "$REPO_ROOT/$_src" "$_out_b"; then
        fail_msg "$_name: saltc exited nonzero on second compilation"
        return
    fi
    if ! cmp -s "$_out_a" "$_out_b"; then
        fail_msg "$_name: nondeterministic output between two compilations"
        return
    fi
    if check_file "$_name" "$_out_a" "$_syms" "$_marker"; then
        pass_msg "$_name ($_src)"
    fi
}

# corrupt_check FILE SYMS [MARKER] -- assert an existing artifact FAILS checks.
corrupt_check() {
    local _file="$1"; _syms="$2"; _marker="${3:-}"
    local _base=$(basename "$_file")
    if check_file "$_base(self-test)" "$_file" "$_syms" "$_marker"; then
        printf 'SELF-TEST FAILURE: %s passed all checks; corruption NOT detected\n' "$_base"
        exit 1
    fi
    printf 'SELF-TEST OK: corruption in %s detected (script would exit nonzero)\n' "$_base"
    exit 0
}

main() {
    if [ "${1:-}" = "--corrupt-check" ]; then
        shift
        corrupt_check "$@"
    fi
    if [ ! -x "$SALTC" ]; then
        printf 'ERROR: saltc binary not found or not executable: %s\n' "$SALTC"
        printf 'Build it first: cd salt-front && cargo build --release\n'
        exit 1
    fi
    WORK_DIR="${TMPDIR:-/tmp}/salt_verify.$$"
    mkdir -m 700 "$WORK_DIR" || exit 1
    export WORK_DIR ERR_LOG="$WORK_DIR/stderr.txt"

    PROGRAMS=(
        'fibonacci|examples/fibonacci.salt|main,main__fib,salt_arena_alloc|'
        'hello_world|examples/hello_world.salt|main,__salt_print_literal|'
        'pattern_matching|examples/pattern_matching.salt|main,main__safe_div,main__area|'
        'structs|examples/structs.salt|main,main__Point__new,main__make_node|'
        'contracts|examples/contracts.salt|main,main__safe_div,__salt_contract_violation|'
        'pipeline|examples/pipeline.salt|main,main__square,main__double|scf[.]'
        'matmul_affine|salt-front/tests/cases/matmul_affine.salt|main|affine[.]for'
        'regression_popcount|salt-front/tests/cases/regression_popcount.salt|main|math[.]'
    )

    for spec in "${PROGRAMS[@]}"; do
        verify_case "$spec"
    done

    printf '\n%d passed, %d failed\n' "$PASS_COUNT" "$FAIL_COUNT"
    rm -rf "$WORK_DIR"
    [ "$FAIL_COUNT" -eq 0 ] && exit 0
    exit 1
}

main "$@"
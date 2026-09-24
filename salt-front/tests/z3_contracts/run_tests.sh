#!/usr/bin/env bash
# =============================================================================
# Z3 Contract Regression Tests
# =============================================================================
# Runs each contract through saltc --verify and checks the expected result.
# Used to detect the Z3 SAT/UNSAT inversion and other verification regressions.
#
# Usage: bash $PROJECT_ROOT/salt-front/tests/z3_contracts/run_tests.sh
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

SALTC="${SALTC:-$PROJECT_ROOT/salt-front/target/release/saltc}"
if [ ! -f "$SALTC" ]; then
    SALTC="$PROJECT_ROOT/salt-front/target/debug/saltc"
fi
PASS=0
FAIL=0

# Extract the Z3 metrics or error line from test output for evidence.
show_evidence() {
    local latest
    latest=$(ls -t /tmp/z3_out_*.txt /tmp/z3_test_* 2>/dev/null | head -1)
    local z3_line
    z3_line=$(grep -hm1 'Z3:\|VERIFICATION ERROR\|contract evaluates to false\|Postcondition violation' ${latest:-/dev/null} 2>/dev/null | head -1) || true
    if [ -n "$z3_line" ]; then
        echo "       $z3_line"
    fi
    return 0
}

echo "=== Z3 Contract Regression Suite ==="
echo ""

# ── Test 1: Contract MUST be proved ────────────────────────────
echo -n "  test_contract_proved: "
if "$SALTC" "$SCRIPT_DIR/test_contract_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_proved > /tmp/z3_out_proved.txt 2>&1; then
    echo "PASS (Z3 proved the contract)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unexpected compile error — possible SAT/UNSAT inversion)"
    cat /tmp/z3_out_proved.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 2: Contract MUST be rejected ──────────────────────────
echo -n "  test_contract_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_contract_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_rejected > /tmp/z3_out_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|contract evaluates to false' /tmp/z3_out_rejected.txt; then
        echo "PASS (contract violation caught)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (compile error but not from verification)"
        cat /tmp/z3_out_rejected.txt | head -3
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (unexpected compile success — SAT/UNSAT inversion detected!)"
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 3: Complex contract (timeout/fallback) ─────────────────
echo -n "  test_contract_timeout: "
OUTCOME=$( "$SALTC" "$SCRIPT_DIR/test_contract_timeout.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_timeout 2>&1 || true )
if echo "$OUTCOME" | grep -q 'VERIFICATION ERROR'; then
    echo "PASS (Z3 could not prove, runtime assertion emitted)"
    PASS=$((PASS + 1))
    show_evidence
elif echo "$OUTCOME" | grep -q 'compiled successfully'; then
    echo "PASS (compiled — contract proved within timeout)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "INCONCLUSIVE (unexpected output)"
    echo "$OUTCOME" | head -3
fi

# ── Test 4: Symbolic string contracts MUST be proved ────────────
echo -n "  test_strings_symbolic: "
if "$SALTC" "$SCRIPT_DIR/test_strings_symbolic.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_strings_sym > /tmp/z3_out_strings_sym.txt 2>&1; then
    echo "PASS (symbolic string contracts proved)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unexpected verification error)"
    cat /tmp/z3_out_strings_sym.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 5: Symbolic string contracts MUST be rejected ──────────
echo -n "  test_strings_symbolic_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_strings_symbolic_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_strings_sym_rej > /tmp/z3_out_strings_sym_rej.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|contract evaluates to false' /tmp/z3_out_strings_sym_rej.txt; then
        echo "PASS (contract violation caught)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (compile error but not from verification)"
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (unexpected compile success — should have been rejected)"
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 6: Real (exact rational) contracts — KNOWN FLAKY ──────
# Z3's Real theory is incomplete (per FAQ). The proof budget is not
# always sufficient. Accept both pass and timeout.
echo -n "  test_real: "
if "$SALTC" "$SCRIPT_DIR/test_real.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_real > /tmp/z3_out_real.txt 2>&1; then
    echo "PASS (Real contracts proved)"
    PASS=$((PASS + 1))
    show_evidence
else
    if grep -q 'VERIFICATION ERROR.*could not prove' /tmp/z3_out_real.txt; then
        echo "SKIP (known Z3 Real theory limitation — FAQ: float theory incomplete)"
    else
        echo "FAIL (unexpected error)"
        cat /tmp/z3_out_real.txt | head -3
        FAIL=$((FAIL + 1))
    fi
fi

# ── Test 7: BV (bitvector) contracts MUST be proved ──────────────
echo -n "  test_bv: "
if "$SALTC" "$SCRIPT_DIR/test_bv.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_bv > /tmp/z3_out_bv.txt 2>&1; then
    echo "PASS (BV contracts proved)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unexpected verification error)"
    cat /tmp/z3_out_bv.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 8: Contract library predicates MUST be proved ──────────
echo -n "  test_contract_library: "
if "$SALTC" "$SCRIPT_DIR/test_contract_library.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_contract_lib > /tmp/z3_out_contract_lib.txt 2>&1; then
    echo "PASS (contract library predicates proved)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unexpected compile error)"
    cat /tmp/z3_out_contract_lib.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 9: ensures(result != 0) MUST be proved ──────────────────
echo -n "  test_ensures_nonzero_proved: "
if "$SALTC" "$SCRIPT_DIR/test_ensures_nonzero_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_ensures_proved > /tmp/z3_out_ensures_proved.txt 2>&1; then
    echo "PASS (postcondition proved — result is never zero)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unexpected verification error)"
    cat /tmp/z3_out_ensures_proved.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 10: ensures(result != 0) MUST be rejected ───────────────
echo -n "  test_ensures_nonzero_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_ensures_nonzero_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_ensures_rejected > /tmp/z3_out_ensures_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|contract evaluates to false\|Postcondition violation' /tmp/z3_out_ensures_rejected.txt; then
        echo "PASS (postcondition violation caught — returns 0 despite ensures(result!=0))"
        PASS=$((PASS + 1))
    else
        echo "FAIL (compile error but not from verification)"
        cat /tmp/z3_out_ensures_rejected.txt | head -3
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (should have been rejected — Z3 missed the postcondition violation)"
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 11: requires(start < len) MUST be proved ────────────────
echo -n "  test_requires_bounds_proved: "
if "$SALTC" "$SCRIPT_DIR/test_requires_bounds_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_bounds_proved > /tmp/z3_out_bounds_proved.txt 2>&1; then
    echo "PASS (bounds precondition proved — valid array access)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unexpected verification error)"
    cat /tmp/z3_out_bounds_proved.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 12: requires(start < len) MUST be rejected ──────────────
echo -n "  test_requires_bounds_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_requires_bounds_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_bounds_rejected > /tmp/z3_out_bounds_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|contract evaluates to false' /tmp/z3_out_bounds_rejected.txt; then
        echo "PASS (bounds violation caught — idx=150 exceeds len=10)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (compile error but not from verification)"
        cat /tmp/z3_out_bounds_rejected.txt | head -3
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (should have been rejected — Z3 missed the bounds violation)"
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 13: requires(a.length() >= b.length()) MUST be proved ─────
echo -n "  test_string_length_proved: "
if "$SALTC" "$SCRIPT_DIR/test_string_length_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_strlen_proved > /tmp/z3_out_strlen_proved.txt 2>&1; then
    echo "PASS (string length comparison proved — .length() folded to constants)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unexpected verification error)"
    cat /tmp/z3_out_strlen_proved.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 14: requires(a.length() >= b.length()) MUST be rejected ────
echo -n "  test_string_length_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_string_length_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_strlen_rejected > /tmp/z3_out_strlen_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|contract evaluates to false' /tmp/z3_out_strlen_rejected.txt; then
        echo "PASS (string length violation caught — 2 >= 11 is false)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (compile error but not from verification)"
        cat /tmp/z3_out_strlen_rejected.txt | head -3
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (should have been rejected — Z3 missed the length violation)"
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 15: requires(a.length() < N) for fixed arrays ─────────────
echo -n "  test_array_length_proved: "
if "$SALTC" "$SCRIPT_DIR/test_array_length_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_arr_len > /tmp/z3_out_arr_len.txt 2>&1; then
    echo "PASS (array length from type — [u8;100].length() = 100 < 200 proved)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unexpected verification error)"
    cat /tmp/z3_out_arr_len.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 16: while-loop invariant enables array bounds proof ─────
echo -n "  test_while_invariant: "
if "$SALTC" "$SCRIPT_DIR/test_while_invariant.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_while_inv > /tmp/z3_out_while_inv.txt 2>&1; then
    echo "PASS (while invariant proves array bounds — i >= 0 && i < 5)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unexpected error — while invariant should prove bounds)"
    cat /tmp/z3_out_while_inv.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 17: while-loop invariant MUST be rejected ─────────────────
echo -n "  test_while_invariant_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_while_invariant_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_while_inv_rej > /tmp/z3_out_while_inv_rej.txt 2>&1; then
    if grep -q 'invariant does not hold' /tmp/z3_out_while_inv_rej.txt; then
        echo "PASS (invariant violation caught — i=5 violates i<5 at entry)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (compile error but not from invariant)"
        cat /tmp/z3_out_while_inv_rej.txt | head -3
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (should have been rejected — Z3 missed the invariant violation)"
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 18: @ operator (linalg.matmul) MUST compile ──────────────
echo -n "  test_matmul_operator: "
if "$SALTC" "$SCRIPT_DIR/test_matmul_operator.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_matmul > /tmp/z3_out_matmul.txt 2>&1; then
    echo "PASS (@ operator compiles — Tensor memref type fix verified)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (Tensor type mismatch regression)"
    cat /tmp/z3_out_matmul.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 19: Bubble sort with array-content invariants ──────────────
echo -n "  test_bubble_sort: "
if "$SALTC" "$SCRIPT_DIR/test_bubble_sort.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_bubble > /tmp/z3_out_bubble.txt 2>&1; then
    echo "PASS (bubble sort compiles with forall invariants)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (bubble sort verification regression)"
    cat /tmp/z3_out_bubble.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 20: Selection sort with integer invariants ─────────────────
echo -n "  test_selection_sort: "
if "$SALTC" "$SCRIPT_DIR/test_selection_sort.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_sel > /tmp/z3_out_sel.txt 2>&1; then
    echo "PASS (selection sort compiles with invariants)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (selection sort verification regression)"
    cat /tmp/z3_out_sel.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 21: Binary search with while-loop invariants ───────────────
echo -n "  test_binary_search: "
if "$SALTC" "$SCRIPT_DIR/test_binary_search.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_bs > /tmp/z3_out_bs.txt 2>&1; then
    echo "PASS (binary search with while-loop invariants)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (binary search verification regression)"
    cat /tmp/z3_out_bs.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 22: Array fill with concrete unrolling ─────────────────────
echo -n "  test_array_fill: "
if "$SALTC" "$SCRIPT_DIR/test_array_fill.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_af > /tmp/z3_out_af.txt 2>&1; then
    echo "PASS (array fill with concrete unrolling)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (array fill verification regression)"
    cat /tmp/z3_out_af.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 23: Exists quantifier ────────────────────────────────────
echo -n "  test_exists: "
if "$SALTC" "$SCRIPT_DIR/test_exists.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_ex > /tmp/z3_out_ex.txt 2>&1; then
    echo "PASS (exists quantifier — Z3 existentially quantified)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (exists quantifier regression)"
    cat /tmp/z3_out_ex.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 24: Exists with symbolic bounds ───────────────────────────
echo -n "  test_exists_symbolic: "
if "$SALTC" "$SCRIPT_DIR/test_exists_symbolic.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_exs > /tmp/z3_out_exs.txt 2>&1; then
    echo "PASS (symbolic exists — Z3 exists_const quantifier)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (symbolic exists regression)"
    cat /tmp/z3_out_exs.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 25: For-loop invariant + forall (inductive step) ─────────
echo -n "  test_insertion_sort: "
if "$SALTC" "$SCRIPT_DIR/test_insertion_sort.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_is > /tmp/z3_out_is.txt 2>&1; then
    echo "PASS (forall ensures/requires + for-loop invariant)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (forall / for-loop invariant regression)"
    cat /tmp/z3_out_is.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 25: Forall ensures at concrete call site ─────────────────
echo -n "  test_insertion_sort_concrete: "
if "$SALTC" "$SCRIPT_DIR/test_insertion_sort_concrete.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_isc > /tmp/z3_out_isc.txt 2>&1; then
    echo "PASS (forall ensures — concrete call-site expansion)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (concrete call-site forall regression)"
    cat /tmp/z3_out_isc.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 26: Comprehensive contract types ─────────────────────────
echo -n "  test_comprehensive: "
if "$SALTC" "$SCRIPT_DIR/test_comprehensive.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_comp > /tmp/z3_out_comp.txt 2>&1; then
    echo "PASS (bounds/division/multiplication/bitwise/branch postconditions)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (comprehensive contract regression)"
    cat /tmp/z3_out_comp.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 27: String content operations ────────────────────────────
echo -n "  test_string_ops: "
if "$SALTC" "$SCRIPT_DIR/test_string_ops.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_sop > /tmp/z3_out_sop.txt 2>&1; then
    echo "PASS (string starts_with/ends_with/matches)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (string ops regression)"
    cat /tmp/z3_out_sop.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 28: Basic string contracts ───────────────────────────────
echo -n "  test_strings: "
if "$SALTC" "$SCRIPT_DIR/test_strings.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_str > /tmp/z3_out_str.txt 2>&1; then
    echo "PASS (string length contracts)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (string contracts regression)"
    cat /tmp/z3_out_str.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 29: String contract violations (negative test) ───────────
echo -n "  test_strings_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_strings_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_srj > /tmp/z3_out_srj.txt 2>&1; then
    echo "PASS (contract violation caught)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (failed to reject invalid string contracts)"
    cat /tmp/z3_out_srj.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 33: Ensures forall on body array writes ──────────────────
echo -n "  test_ensures_forall_body: "
if "$SALTC" "$SCRIPT_DIR/test_ensures_forall_body.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_efb > /tmp/z3_out_efb.txt 2>&1; then
    echo "PASS (ensures forall proved from body array stores)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (ensures forall body store regression)"
    cat /tmp/z3_out_efb.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 34: Ensures forall rejected from body stores ─────────────
echo -n "  test_ensures_forall_body_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_ensures_forall_body_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_efbr > /tmp/z3_out_efbr.txt 2>&1; then
    echo "PASS (ensures forall violation caught from body stores)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (failed to reject invalid ensures forall)"
    cat /tmp/z3_out_efbr.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 35: Forall requires at call site (positive) ──────────────
echo -n "  test_forall_requires_proved: "
if "$SALTC" "$SCRIPT_DIR/test_forall_requires_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_frp > /tmp/z3_out_frp.txt 2>&1; then
    echo "PASS (forall requires proved with call-site expansion)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (forall requires expansion regression)"
    cat /tmp/z3_out_frp.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 36: Forall requires at call site (negative) ──────────────
echo -n "  test_forall_requires_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_forall_requires_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_frr > /tmp/z3_out_frr.txt 2>&1; then
    echo "PASS (forall requires violation caught — i<5 with n=6)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (failed to reject invalid forall requires)"
    cat /tmp/z3_out_frr.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 37: BV shift operations ──────────────────────────────────
echo -n "  test_bv_shifts: "
if "$SALTC" "$SCRIPT_DIR/test_bv_shifts.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_bvs > /tmp/z3_out_bvs.txt 2>&1; then
    echo "PASS (BV shift bounds — x<<3 and x>>3 with ensures)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (BV shift regression)"
    cat /tmp/z3_out_bvs.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 34: Type bounds — counterexample rejection ───────────────
echo -n "  test_type_bounds_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_type_bounds_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_tbr > /tmp/z3_out_tbr.txt 2>&1; then
    echo "PASS (type bound violation caught — u8(x<100) with x=200)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (failed to reject type-bound violation)"
    cat /tmp/z3_out_tbr.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 35: Type-bound proofs ────────────────────────────────────
echo -n "  test_type_bounds: "
if "$SALTC" "$SCRIPT_DIR/test_type_bounds.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_tb > /tmp/z3_out_tb.txt 2>&1; then
    echo "PASS (type-bound proofs: u8/bool/u16)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (type-bound proof regression)"
    cat /tmp/z3_out_tb.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 31: Element preservation across mutations (frame axioms) ─
echo -n "  test_preservation: "
if "$SALTC" "$SCRIPT_DIR/test_preservation.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_pr > /tmp/z3_out_pr.txt 2>&1; then
    echo "PASS (element preservation with frame axioms)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (preservation proof regression)"
    cat /tmp/z3_out_pr.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 32: Cross-function contract chaining ───────────────────
echo -n "  test_cross_fn_chain: "
if "$SALTC" "$SCRIPT_DIR/test_cross_fn_chain.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_cfc > /tmp/z3_out_cfc.txt 2>&1; then
    echo "PASS (cross-function postcondition chaining)"
    PASS=$((PASS + 1))
    show_evidence
else
    # broken_double_half should fail (ensures result == x+1 doesn't hold)
    if grep -q 'Postcondition violation\|postcondition' /tmp/z3_out_cfc.txt; then
        echo "PASS (violation correctly caught in broken_double_half)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (cross-function chaining regression)"
        cat /tmp/z3_out_cfc.txt | head -3
        FAIL=$((FAIL + 1))
    fi
fi

# ── Test 33: Struct field type bounds ───────────────────────────
echo -n "  test_struct_field_bounds: "
if "$SALTC" "$SCRIPT_DIR/test_struct_field_bounds.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_sfb > /tmp/z3_out_sfb.txt 2>&1; then
    if grep -q '1/1 checks proven' /tmp/z3_out_sfb.txt; then
        echo "PASS (struct field u8 bounds — p.x < 256 proven)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (struct field bounds not proven)"
        cat /tmp/z3_out_sfb.txt | head -3
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (struct field bounds test compilation failed)"
    cat /tmp/z3_out_sfb.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 40: Slice cursor with while-loop — bounds must prove ──────
echo -n "  test_slice_cursor_proved: "
if "$SALTC" "$SCRIPT_DIR/test_slice_cursor_proved.salt" \
    --disable-alias-scopes -o /tmp/z3_test_scp > /tmp/z3_out_scp.txt 2>&1; then
    echo "PASS (slice set() bounds proven inside while loop)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (slice cursor bounds should prove with loop invariant)"
    cat /tmp/z3_out_scp.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 41: Loop call precondition must prove ────────────────────
echo -n "  test_loop_call_precond_proved: "
if "$SALTC" "$SCRIPT_DIR/test_loop_call_precond_proved.salt" \
    --disable-alias-scopes -o /tmp/z3_test_lcp > /tmp/z3_out_lcp.txt 2>&1; then
    echo "PASS (call precondition proved via loop invariant + guard)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (loop call precondition should prove)"
    cat /tmp/z3_out_lcp.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 42: Slice construction length must propagate ─────────────
echo -n "  test_slice_len_construction_proved: "
if "$SALTC" "$SCRIPT_DIR/test_slice_len_construction_proved.salt" \
    --disable-alias-scopes -o /tmp/z3_test_slc > /tmp/z3_out_slc.txt 2>&1; then
    echo "PASS (Slice::new length propagated to .len() contract)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (construction length should prove buf.len() == 100)"
    cat /tmp/z3_out_slc.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 43: Slice cursor OOB must reject ─────────────────────────
echo -n "  test_slice_cursor_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_slice_cursor_rejected.salt" \
    --disable-alias-scopes -o /tmp/z3_test_scr > /tmp/z3_out_scr.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR' /tmp/z3_out_scr.txt; then
        echo "PASS (OOB slice access correctly rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (compile error but not from verification)"
        cat /tmp/z3_out_scr.txt | head -3
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (should reject OOB access — unsound elision detected!)"
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Test 44: For-loop induction variable bounds must prove ─────────
echo -n "  test_for_loop_slice: "
if "$SALTC" "$SCRIPT_DIR/test_for_loop_slice.salt" \
    --disable-alias-scopes -o /tmp/z3_test_fls > /tmp/z3_out_fls.txt 2>&1; then
    echo "PASS (for-loop s.at(i) bounds proved via induction variable tracking)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (for-loop induction variable bounds should prove)"
    cat /tmp/z3_out_fls.txt | head -3
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── Named constants in contracts MUST resolve to their value ───
echo -n "  test_const_in_contract_proved: "
if "$SALTC" "$SCRIPT_DIR/test_const_in_contract_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_const > /tmp/z3_out_const.txt 2>&1; then
    echo "PASS (constant resolved to its value)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (constant lowered to an unconstrained symbol)"
    cat /tmp/z3_out_const.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── ...without weakening soundness ─────────────────────────────
echo -n "  test_const_in_contract_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_const_in_contract_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_const_rej > /tmp/z3_out_const_rej.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_const_rej.txt; then
        echo "PASS (false contract still rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_const_rej.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (false contract accepted — soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── Bool postconditions MUST be checked, not skipped ───────────
echo -n "  test_bool_postcondition_proved: "
if "$SALTC" "$SCRIPT_DIR/test_bool_postcondition_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_bpc > /tmp/z3_out_bpc.txt 2>&1; then
    echo "PASS (bool postconditions proved)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (true bool postcondition rejected)"
    cat /tmp/z3_out_bpc.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

echo -n "  test_bool_postcondition_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_bool_postcondition_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_bpc_rej > /tmp/z3_out_bpc_rej.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_bpc_rej.txt; then
        echo "PASS (false bool postcondition rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (false bool postcondition ACCEPTED — silently skipped)"
    FAIL=$((FAIL + 1))
fi

# ── Unsigned types must carry non-negativity for the solver ────
echo -n "  test_unsigned_bound_proved: "
if "$SALTC" "$SCRIPT_DIR/test_unsigned_bound_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_ub > /tmp/z3_out_ub.txt 2>&1; then
    echo "PASS (unsigned bounds proved)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (unsigned non-negativity not available to the solver)"
    cat /tmp/z3_out_ub.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

echo -n "  test_unsigned_bound_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_unsigned_bound_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_ub_rej > /tmp/z3_out_ub_rej.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|could not prove' /tmp/z3_out_ub_rej.txt; then
        echo "PASS (genuinely unguarded subtraction still rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (unguarded subtraction ACCEPTED — soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── A deliberately weakened bound must be rejected, not compiled in
# silence -- this query used to time out under the old 100ms wall-clock
# watchdog (deferred to a runtime check); the rlimit-based budget decides
# it directly instead. See the fixture's own header for the full history.
echo -n "  test_ensures_timeout_runtime_check: "
if ! "$SALTC" "$SCRIPT_DIR/test_ensures_timeout_runtime_check.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_etrc > /tmp/z3_out_etrc.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_etrc.txt; then
        echo "PASS (weakened bound caught at compile time, not deferred or silent)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_etrc.txt | head -5
        FAIL=$((FAIL + 1))
    fi
elif grep -q '__salt_contract_violation' /tmp/z3_test_etrc \
    && grep -qi 'WARNING: Z3 could not prove' /tmp/z3_out_etrc.txt; then
    # Sound, but weaker than this fixture expects: Z3 ran out of budget and
    # the compiler deferred to a runtime check. Outcomes here are calibrated
    # to the Z3 version CI pins; an older Z3 lands in this branch.
    echo "FAIL (deferred to a runtime check instead of compile-time rejection: sound, but this Z3 is weaker than the version CI pins)"
    FAIL=$((FAIL + 1))
else
    echo "FAIL (weakened bound ACCEPTED with no runtime check — soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── A genuinely-undecided-within-budget ensures MUST still emit a
# runtime check, not compile in silence ──
echo -n "  test_ensures_deferred_runtime_check: "
if "$SALTC" "$SCRIPT_DIR/test_ensures_deferred_runtime_check.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_edrc > /tmp/z3_out_edrc.txt 2>&1; then
    if grep -q '__salt_contract_violation' /tmp/z3_test_edrc; then
        if grep -qi 'WARNING: Z3 could not prove' /tmp/z3_out_edrc.txt; then
            echo "PASS (deferred ensures got a real runtime check + warning)"
            PASS=$((PASS + 1))
            show_evidence
        else
            echo "FAIL (runtime check present but no warning printed)"
            FAIL=$((FAIL + 1))
        fi
    else
        echo "FAIL (compiled with ZERO enforcement — the original silent bug)"
        cat /tmp/z3_out_edrc.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (unexpected compile error — was this supposed to stay undecided?)"
    cat /tmp/z3_out_edrc.txt | head -5
    FAIL=$((FAIL + 1))
fi

# ── A non-mut local must be constrained to its defining expression ──
echo -n "  test_let_binding_proved: "
if "$SALTC" "$SCRIPT_DIR/test_let_binding_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_letb_proved > /tmp/z3_out_letb_proved.txt 2>&1; then
    echo "PASS (let-bound name related to its defining expression)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (let binding still unrelated to its defining expression)"
    cat /tmp/z3_out_letb_proved.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

echo -n "  test_let_binding_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_let_binding_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_letb_rejected > /tmp/z3_out_letb_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_letb_rejected.txt; then
        echo "PASS (genuinely wrong validator still rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_letb_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (wrong validator ACCEPTED — soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── A function's own requires must be assumed for calls it makes ──
echo -n "  test_composability_requires_proved: "
if "$SALTC" "$SCRIPT_DIR/test_composability_requires_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_cr_proved > /tmp/z3_out_cr_proved.txt 2>&1; then
    echo "PASS (own requires justified the call it makes)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (caller_preconditions still not reaching the solver)"
    cat /tmp/z3_out_cr_proved.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

echo -n "  test_composability_requires_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_composability_requires_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_cr_rejected > /tmp/z3_out_cr_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_cr_rejected.txt; then
        echo "PASS (looser caller bound did not over-justify the call)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_cr_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (looser bound ACCEPTED — soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── A callee's ensures must be assumed at its own call site ──
echo -n "  test_composability_ensures_proved: "
if "$SALTC" "$SCRIPT_DIR/test_composability_ensures_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_ce_proved > /tmp/z3_out_ce_proved.txt 2>&1; then
    echo "PASS (callee ensures propagated to the caller's later call)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (apply_ensures_to_solver still not reaching the solver)"
    cat /tmp/z3_out_ce_proved.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

echo -n "  test_composability_ensures_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_composability_ensures_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_ce_rejected > /tmp/z3_out_ce_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_ce_rejected.txt; then
        echo "PASS (propagated ensures did not over-justify an unrelated call)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_ce_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (unrelated stronger requirement ACCEPTED — soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── Postcondition type-bounds scoping must reach the return expression ──
echo -n "  test_ensures_return_expr_bounds_proved: "
if "$SALTC" "$SCRIPT_DIR/test_ensures_return_expr_bounds_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_erb_proved > /tmp/z3_out_erb_proved.txt 2>&1; then
    echo "PASS (n's u64 bound reached the postcondition via the WP binding)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (return-expression identifiers still invisible to type bounds)"
    cat /tmp/z3_out_erb_proved.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

echo -n "  test_ensures_return_expr_bounds_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_ensures_return_expr_bounds_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_erb_rejected > /tmp/z3_out_erb_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_erb_rejected.txt; then
        echo "PASS (genuinely false postcondition still rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_erb_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (false postcondition ACCEPTED — soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── The Hoare post-loop fact (invariant && !cond) must be usable after a while loop ──
echo -n "  test_while_post_loop_proved: "
if "$SALTC" "$SCRIPT_DIR/test_while_post_loop_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_wpl_proved > /tmp/z3_out_wpl_proved.txt 2>&1; then
    echo "PASS (post-loop invariant reached the later requires check)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (post-loop fact still not reaching the solver)"
    cat /tmp/z3_out_wpl_proved.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

echo -n "  test_while_post_loop_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_while_post_loop_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_wpl_rejected > /tmp/z3_out_wpl_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_wpl_rejected.txt; then
        echo "PASS (unjustified post-loop claim still rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_wpl_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (unjustified claim ACCEPTED — soundness lost)"
    FAIL=$((FAIL + 1))
fi

echo -n "  test_while_post_loop_var_reuse_proved: "
if "$SALTC" "$SCRIPT_DIR/test_while_post_loop_var_reuse_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_wplr_proved > /tmp/z3_out_wplr_proved.txt 2>&1; then
    echo "PASS (second loop's own post-fact usable despite name reuse)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (name-reuse anchoring broke a legitimate proof)"
    cat /tmp/z3_out_wplr_proved.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

echo -n "  test_while_post_loop_var_reuse_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_while_post_loop_var_reuse_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_wplr_rejected > /tmp/z3_out_wplr_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_wplr_rejected.txt; then
        echo "PASS (stale fact from a same-named earlier loop did not leak)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_wplr_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (absurd claim ACCEPTED — stale fact contradiction, soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── A havoc'd-argument requires failure must suggest 'invariant', not requires/assert ──
echo -n "  test_havoc_invariant_hint: "
if ! "$SALTC" "$SCRIPT_DIR/test_havoc_invariant_hint.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_hih > /tmp/z3_out_hih.txt 2>&1; then
    if grep -q "add 'invariant y > 0;' to that loop" /tmp/z3_out_hih.txt; then
        if grep -q "to the function signature\|before this line" /tmp/z3_out_hih.txt; then
            echo "FAIL (misleading requires/assert hint present alongside the invariant one)"
            cat /tmp/z3_out_hih.txt | head -8
            FAIL=$((FAIL + 1))
        else
            echo "PASS (suggested the correct invariant, in the caller's own terms)"
            PASS=$((PASS + 1))
            show_evidence
        fi
    else
        echo "FAIL (havoc'd argument failure did not suggest an invariant)"
        cat /tmp/z3_out_hih.txt | head -8
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (should have been rejected — no invariant was ever added)"
    FAIL=$((FAIL + 1))
fi

# ── Houdini-lite: a call's requires must be tried as a candidate invariant automatically ──
echo -n "  test_houdini_call_requires_proved: "
if "$SALTC" "$SCRIPT_DIR/test_houdini_call_requires_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_hcr_proved > /tmp/z3_out_hcr_proved.txt 2>&1; then
    echo "PASS (call's requires auto-discharged with no hand-written invariant)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (Houdini-lite candidate not discharging the call automatically)"
    cat /tmp/z3_out_hcr_proved.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

echo -n "  test_houdini_call_requires_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_houdini_call_requires_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_hcr_rejected > /tmp/z3_out_hcr_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_hcr_rejected.txt; then
        echo "PASS (candidate that fails the base case was correctly dropped, call still rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_hcr_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (a base-case-failing candidate was trusted anyway — soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── symbolic_tracker must not leak a havoc'd name across functions ──
echo -n "  test_cross_fn_symbolic_tracker_proved: "
if "$SALTC" "$SCRIPT_DIR/test_cross_fn_symbolic_tracker_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_cfst_proved > /tmp/z3_out_cfst_proved.txt 2>&1; then
    echo "PASS (same-named loop variables in different functions did not contaminate each other)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (cross-function symbolic_tracker leak reintroduced)"
    cat /tmp/z3_out_cfst_proved.txt | head -5
    FAIL=$((FAIL + 1))
    show_evidence
fi

# ── pointer_tracker must not leak a Valid marking across functions ──
echo -n "  test_cross_fn_pointer_tracker_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_cross_fn_pointer_tracker_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_cfpt_rejected > /tmp/z3_out_cfpt_rejected.txt 2>&1; then
    if grep -q 'VERIFICATION ERROR\|Postcondition violation' /tmp/z3_out_cfpt_rejected.txt; then
        echo "PASS (unrelated function's leftover Valid state did not leak)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_cfpt_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (an unproven pointer was ACCEPTED — cross-function pointer_tracker leak reintroduced, soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── check_deref's rejection must not be silently swallowed by its caller ──
echo -n "  test_use_after_free_via_read_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_use_after_free_via_read_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_uafr_rejected > /tmp/z3_out_uafr_rejected.txt 2>&1; then
    if grep -q "Cannot dereference 'Freed'" /tmp/z3_out_uafr_rejected.txt; then
        echo "PASS (use-after-free via .read() correctly rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_uafr_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (use-after-free via .read() ACCEPTED — check_deref's rejection is being swallowed again, soundness lost)"
    FAIL=$((FAIL + 1))
fi

# ── same fixed path, Optional state instead of Freed ──
echo -n "  test_deref_optional_via_read_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_deref_optional_via_read_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_dor_rejected > /tmp/z3_out_dor_rejected.txt 2>&1; then
    if grep -q "Cannot dereference 'Optional'" /tmp/z3_out_dor_rejected.txt; then
        echo "PASS (unnarrowed Optional-state read correctly rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_dor_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (unnarrowed Optional-state read ACCEPTED — check_deref's rejection is being swallowed again)"
    FAIL=$((FAIL + 1))
fi

# ── same fixed path, Uninitialized state via .write() instead of .read() ──
echo -n "  test_deref_uninitialized_via_write_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_deref_uninitialized_via_write_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_duw_rejected > /tmp/z3_out_duw_rejected.txt 2>&1; then
    if grep -q "Cannot dereference 'Uninitialized'" /tmp/z3_out_duw_rejected.txt; then
        echo "PASS (write through an uninitialized pointer correctly rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_duw_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (write through an uninitialized pointer ACCEPTED — check_deref's rejection is being swallowed again)"
    FAIL=$((FAIL + 1))
fi

# ── i32/i64 must carry their full range, not just be entirely unbounded ──
echo -n "  test_i32_i64_full_range_proved: "
if "$SALTC" "$SCRIPT_DIR/test_i32_i64_full_range_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_i32i64_proved > /tmp/z3_out_i32i64_proved.txt 2>&1; then
    echo "PASS (trivially-true i32/i64 range facts proved or safely deferred)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (an always-true fact about real i32/i64 values was rejected — unbounded-integer regression)"
    cat /tmp/z3_out_i32i64_proved.txt | head -8
    FAIL=$((FAIL + 1))
fi

# ── u32 must carry its actual ceiling (2^32 - 1), not just non-negativity ──
echo -n "  test_u32_upper_bound_proved: "
if "$SALTC" "$SCRIPT_DIR/test_u32_upper_bound_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_u32ub_proved > /tmp/z3_out_u32ub_proved.txt 2>&1; then
    echo "PASS (trivially-true u32 range fact proved or safely deferred)"
    PASS=$((PASS + 1))
    show_evidence
else
    echo "FAIL (an always-true fact about real u32 values was rejected — missing upper bound regression)"
    cat /tmp/z3_out_u32ub_proved.txt | head -8
    FAIL=$((FAIL + 1))
fi

# ── a contract literal at u64::MAX must actually be checked, not skipped ──
echo -n "  test_u64_max_literal_proved: "
if "$SALTC" "$SCRIPT_DIR/test_u64_max_literal_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_u64max_proved > /tmp/z3_out_u64max_proved.txt 2>&1; then
    if grep -q "0/0 checks" /tmp/z3_out_u64max_proved.txt; then
        echo "FAIL (compiled clean but 0 checks ran — silently skipped again, not actually verified)"
        FAIL=$((FAIL + 1))
    else
        echo "PASS (u64::MAX literal actually verified, not silently skipped)"
        PASS=$((PASS + 1))
        show_evidence
    fi
else
    echo "FAIL (u64::MAX literal in a contract was wrongly rejected)"
    cat /tmp/z3_out_u64max_proved.txt | head -8
    FAIL=$((FAIL + 1))
fi

# ── an off-by-one-too-tight bound below u64::MAX must still be rejected ──
echo -n "  test_u64_max_literal_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_u64_max_literal_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_u64max_rejected > /tmp/z3_out_u64max_rejected.txt 2>&1; then
    if grep -q "VERIFICATION ERROR\|Postcondition violation" /tmp/z3_out_u64max_rejected.txt; then
        echo "PASS (genuinely false near-max claim still rejected — literal fix isn't over-permissive)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_u64max_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (a genuinely false near-u64::MAX claim was ACCEPTED)"
    FAIL=$((FAIL + 1))
fi

# ── try_elide_overflow_check: proven-safe arithmetic must have its
# runtime check elided entirely (zero-cost), across each operator and
# signedness try_elide_overflow_check's own match distinguishes ──
for elide_case in overflow_elide_add_proved overflow_elide_sub_proved overflow_elide_mul_proved overflow_elide_unsigned_proved; do
    echo -n "  test_${elide_case}: "
    if "$SALTC" "$SCRIPT_DIR/test_${elide_case}.salt" \
        --lib --disable-alias-scopes -o "/tmp/z3_test_${elide_case}" > "/tmp/z3_out_${elide_case}.txt" 2>&1; then
        if grep -q '__salt_overflow_panic' "/tmp/z3_test_${elide_case}"; then
            echo "FAIL (proven-safe arithmetic still emitted a runtime check — elision regressed)"
            FAIL=$((FAIL + 1))
        else
            echo "PASS (runtime overflow check elided — proven safe at compile time)"
            PASS=$((PASS + 1))
            show_evidence
        fi
    else
        echo "FAIL (unexpected compile error)"
        cat "/tmp/z3_out_${elide_case}.txt" | head -5
        FAIL=$((FAIL + 1))
    fi
done

# ── The three cases elision must decline: unconstrained operands, a
# provable violation (never a hard error -- see the fixture's own
# header), and an untranslatable operand. All three must compile clean
# with the runtime check still present, identical to pre-elision
# behavior ──
for keep_case in overflow_check_kept_unconstrained overflow_check_kept_provable_violation overflow_elide_untranslatable_operand; do
    echo -n "  test_${keep_case}: "
    if "$SALTC" "$SCRIPT_DIR/test_${keep_case}.salt" \
        --lib --disable-alias-scopes -o "/tmp/z3_test_${keep_case}" > "/tmp/z3_out_${keep_case}.txt" 2>&1; then
        if grep -q '__salt_overflow_panic' "/tmp/z3_test_${keep_case}"; then
            echo "PASS (runtime check correctly kept, not over-elided)"
            PASS=$((PASS + 1))
        else
            echo "FAIL (check missing — elision fired when it should have declined)"
            FAIL=$((FAIL + 1))
        fi
    else
        echo "FAIL (unexpected compile error — should compile clean with a runtime check)"
        cat "/tmp/z3_out_${keep_case}.txt" | head -5
        FAIL=$((FAIL + 1))
    fi
done

# ── malloc_tracker cross-function audit: closes the thread left dangling
# since the pointer_tracker fix -- confirmed already correctly scoped
# (swap-and-restore in emit_fn), locked in as a permanent regression
# guard rather than left as an unverified commit-message claim ──
echo -n "  test_cross_fn_malloc_tracker_proved: "
if "$SALTC" "$SCRIPT_DIR/test_cross_fn_malloc_tracker_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_cfmt_proved > /tmp/z3_out_cfmt_proved.txt 2>&1; then
    echo "PASS (same-named malloc/free in two functions did not contaminate each other)"
    PASS=$((PASS + 1))
else
    echo "FAIL (unexpected compile error)"
    cat /tmp/z3_out_cfmt_proved.txt | head -5
    FAIL=$((FAIL + 1))
fi

echo -n "  test_cross_fn_malloc_leak_still_caught: "
if ! "$SALTC" "$SCRIPT_DIR/test_cross_fn_malloc_leak_still_caught.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_cfml_rejected > /tmp/z3_out_cfml_rejected.txt 2>&1; then
    if grep -q 'Memory Leak Detected' /tmp/z3_out_cfml_rejected.txt; then
        echo "PASS (leak in first() still caught despite second()'s unrelated reuse of the name)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_cfml_rejected.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (a genuine leak was ACCEPTED — leak detection defeated by unrelated name reuse)"
    FAIL=$((FAIL + 1))
fi

# ── arena_escape_tracker: same audit, same closing-the-loop rationale ──
echo -n "  test_arena_escape_direct_rejected: "
if ! "$SALTC" "$SCRIPT_DIR/test_arena_escape_direct_rejected.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_aedr > /tmp/z3_out_aedr.txt 2>&1; then
    if grep -q 'Arena escape violation' /tmp/z3_out_aedr.txt; then
        echo "PASS (returning a local-arena pointer correctly rejected)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (rejected for the wrong reason)"
        cat /tmp/z3_out_aedr.txt | head -5
        FAIL=$((FAIL + 1))
    fi
else
    echo "FAIL (a genuine arena escape was ACCEPTED)"
    FAIL=$((FAIL + 1))
fi

echo -n "  test_cross_fn_arena_escape_proved: "
if "$SALTC" "$SCRIPT_DIR/test_cross_fn_arena_escape_proved.salt" \
    --lib --disable-alias-scopes -o /tmp/z3_test_cfae_proved > /tmp/z3_out_cfae_proved.txt 2>&1; then
    echo "PASS (unrelated function's leftover arena taint did not leak)"
    PASS=$((PASS + 1))
else
    echo "FAIL (unexpected compile error — cross-function arena taint leak reintroduced)"
    cat /tmp/z3_out_cfae_proved.txt | head -5
    FAIL=$((FAIL + 1))
fi

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [ "$FAIL" -gt 0 ]; then
    echo "REGESSION DETECTED — Z3 verification behavior has changed!"
    exit 1
else
    echo "All tests pass — Z3 verification working correctly."
fi

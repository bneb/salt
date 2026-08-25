#!/usr/bin/env bash
# Spelling-goldens gate (WS-R Sprint-0 lock-in).
#
# Pins the exact emitted symbol/type spellings of the RT6 probe battery so
# identity/naming regressions fail CI instead of shipping. Each row carries
# an OWNER tag: rows marked FLIP[T-a]/FLIP[T-b] are EXPECTED to change when
# those WS-R3 tickets land (see .round1-staging/handoff_note.md) -- update
# this table in the SAME commit as the behavior change, never before.
#
# Usage: bash scripts/spelling_goldens_gate.sh
# Exit codes: 0 all goldens hold / 1 spelling drift or unexpected outcome.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SALTC="${SALTC:-$ROOT/salt-front/target/release/saltc}"
RT="$ROOT/.round1-staging/wsr2/rt"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

FAILURES=""

# check <name> <expected_exit:0|nonzero> <require_regex> <forbid_regex> <probe>
check() {
  local name="$1" want_exit="$2" require="$3" forbid="$4" probe="$5"
  local out="$TMP/${name}.mlir" rc=0
  "$SALTC" "$probe" -o "$out" >/dev/null 2>&1 || rc=$?
  if [ "$want_exit" = "0" ] && [ "$rc" -ne 0 ]; then
    FAILURES+="GOLDEN $name: expected exit 0, got $rc"$'\n'; return
  fi
  if [ "$want_exit" = "nonzero" ] && [ "$rc" -eq 0 ]; then
    FAILURES+="GOLDEN $name: expected nonzero exit, got 0"$'\n'; return
  fi
  if [ -n "$require" ] && ! grep -qE "$require" "$out" 2>/dev/null; then
    FAILURES+="GOLDEN $name: required pattern missing: $require"$'\n'
  fi
  if [ -n "$forbid" ] && grep -qE "$forbid" "$out" 2>/dev/null; then
    FAILURES+="GOLDEN $name: forbidden pattern present: $forbid"$'\n'
  fi
}

# Baselines (stable since WS-2 family closure, rounds 2-4).
check cache_repro 0 'Cache__new_64' '_SIZE' \
  "$ROOT/.round1-staging/const_generic_overflow_repro.salt"
check shadow_repro 0 'Box2__make_5' '(_NODE|Node_[0-9])' \
  "$ROOT/.round1-staging/ws2-tail/adv_shadow_real_struct.salt"

# RT6 probes. FLIP rows change in WS-R3 -- see header.
check neg_turbofish 0 'S_-7' 'S_main__-' "$RT/rt_probe_neg_turbofish.salt"  # FLIP[T-a] LANDED: single identity main__S_-7; ghost family forbidden
check neg_two_values 0 'struct_main__S_-13' 'S_main__-' \
  "$ROOT/.round1-staging/wsr3p2/rt_probe_neg_turbofish_two_values.salt"     # FLIP[T-a] stronger golden (RT req): -7 AND -13 => one decl each
check neg_two_values_7 0 'struct_main__S_-7' 'S_main__-' \
  "$ROOT/.round1-staging/wsr3p2/rt_probe_neg_turbofish_two_values.salt"     # symmetric require leg (grep -qE cannot demand BOTH in one row)
check digit_ident_rail 0 'main__S_7' '' \
  "$ROOT/.round1-staging/wsr3p2/rt_probe_digit_ident_regression.salt"       # guard must not over-reject digit-containing idents
check hex_norm 0 'struct_main__S_16' 'S_main__-' \
  "$ROOT/.round1-staging/wsr3p2/rtv_hex_lit.salt"                           # hex literal normalizes via evaluator value
check underscore_norm 0 'struct_main__S_1000' '' \
  "$ROOT/.round1-staging/wsr3p2/rtv_underscore_lit.salt"                    # underscore literal normalizes via evaluator value
check leading_zero 0 '(Z_7|Z__mk_7)' '' "$RT/rt_probe_leading_zero.salt"
check unary_plus nonzero '' '' "$RT/rt_probe_unary_plus.salt"              # parser E002; router unreached
check overflow_literal nonzero '' '' "$RT/rt_probe_overflow_literal.salt"   # FLIP[T-b] LANDED: [E003] refusal cites digits; no MLIR artifact -> no O_K ghost, no ptr-typed call
check i64max 0 '9223372036854775807' '' "$RT/rt_probe_i64max.salt"

if [ -n "$FAILURES" ]; then
  echo "SPELLING-GOLDENS FAILED:"
  printf '%s' "$FAILURES"
  exit 1
fi
echo "SPELLING-GOLDENS OK: all pinned expectations hold"

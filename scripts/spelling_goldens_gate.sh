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
  local out="$TMP/${name}.mlir" err="$TMP/${name}.err" rc=0
  "$SALTC" "$probe" -o "$out" >/dev/null 2>"$err" || rc=$?
  if [ "$want_exit" = "0" ] && [ "$rc" -ne 0 ]; then
    FAILURES+="GOLDEN $name: expected exit 0, got $rc"$'\n'; return
  fi
  if [ "$want_exit" = "nonzero" ] && [ "$rc" -eq 0 ]; then
    FAILURES+="GOLDEN $name: expected nonzero exit, got 0"$'\n'; return
  fi
  # Refusal rows (nonzero) pin their contract against STDERR; compile rows
  # pin against emitted MLIR.
  local target="$out"
  if [ "$want_exit" = "nonzero" ]; then target="$err"; fi
  if [ -n "$require" ] && ! grep -qE "$require" "$target" 2>/dev/null; then
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

# NB-4/NB-5 landed (round 14-15): generic-receiver methods that cannot bind
# their type params REFUSE with actionable guidance instead of wrong-code
# casts/ghosts. Full local inference = backlog WS-7; these rows pin the
# refusal contract until then.
S36="$ROOT/.round1-staging/s3b"
check nb4_infer_refusal nonzero 'Unresolved generic' '' "$S36/nb4_generic_vec_receiver.salt"  # completeness refusal precedes cast diag (NB-5 order)
check nb2_infer_refusal nonzero 'Unresolved generic' '' "$S36/nb2_vec_match_generic.salt"

# T-c landed (round 9): non-Integer turbofish consts get DISTINCT value
# identities -- Bool keywords, Float digit-safe IEEE bits. The gate must
# enforce distinct SUFFIXED CALLS, not just decls (narrow fixes proved
# false-fixed before; see handoff ROUND 8 T-c scope).
check bool_true 0 'struct_main__B_true' '(B_FLAG|B__mk\(\))' \
  "$ROOT/.round1-staging/wsr3/rt_audit_bool_float_turbofish.salt"
check bool_false 0 'struct_main__B_false' '(B_FLAG|B__mk\(\))' \
  "$ROOT/.round1-staging/wsr3/rt_audit_bool_float_turbofish.salt"
check float_distinct 0 'F_4612811918334230528' '(F_D|B__mk\(\))' \
  "$ROOT/.round1-staging/wsr3p2/rtv_float_turbofish.salt"
check float_second 0 'F_4609434218613702656' '' \
  "$ROOT/.round1-staging/wsr3p2/rtv_float_turbofish.salt"

if [ -n "$FAILURES" ]; then
  echo "SPELLING-GOLDENS FAILED:"
  printf '%s' "$FAILURES"
  exit 1
fi
echo "SPELLING-GOLDENS OK: all pinned expectations hold"

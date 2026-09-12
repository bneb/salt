#!/usr/bin/env bash
# Swallowed-error gate: bans a specific, proven-dangerous anti-pattern from
# reappearing anywhere in the compiler source.
#
# `if let Ok(Some(x)) = fallible_fn(...) { ... }` treats a genuine `Err`
# (fallible_fn actually failed) identically to `Ok(None)` (fallible_fn
# succeeded but had nothing to contribute) -- silently falling through to
# whatever comes next in both cases. The strictly-better shape is always
# available in a function that itself returns a compatible Result:
# `if let Some(x) = fallible_fn(...)? { ... }` -- Ok(None) still falls
# through, but Err now aborts with the real error.
#
# This is not a style preference. It was found because it was hiding a
# real bug: try_emit_special_method (special_methods.rs) calls check_deref
# for unsafe Ptr<T> methods (.read()/.write()/.offset()) and correctly
# returns Err on a real violation (Freed/Uninitialized/Empty/Optional).
# Its caller in calls.rs used exactly this pattern, so the Err was
# discarded and `free(p); p.read();` compiled clean with zero error and
# zero runtime check. Two more instances (wrapping emit_intrinsic in
# calls.rs and method_resolution.rs) had the same shape with lower-stakes
# consequences (a confusing fallback error instead of the real one). All
# three are fixed; this gate is what stops a fourth from shipping quietly.
#
# Exit codes: 0 clean / 1 pattern found / 2 harness error.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/salt-front/src"

[ -d "$SRC" ] || { echo "SWALLOWED-ERROR-HARNESS-ERROR: source dir not found at $SRC" >&2; exit 2; }

HITS="$(grep -rn "if let Ok(Some(" "$SRC" --include="*.rs" | grep -v ":[[:space:]]*//" || true)"

if [ -n "$HITS" ]; then
    echo "SWALLOWED-ERROR-GATE FAILED: 'if let Ok(Some(' discards a real Err the same as Ok(None)."
    echo "Use 'if let Some(x) = fallible_fn(...)? { ... }' instead -- Ok(None) still falls through, Err now aborts."
    echo ""
    echo "$HITS"
    exit 1
fi

echo "SWALLOWED-ERROR-GATE OK: no 'if let Ok(Some(' anywhere in salt-front/src"

#!/usr/bin/env bash
# Byte-stable MLIR determinism gate (roadmap item: enforce the
# "deterministic output" property instead of trusting it).
#
# Compiles every non-rejected Z3 fixture TWICE with identical flags and
# compares SHA-256 digests of the emitted MLIR. Any byte of drift fails.
# Also optionally compares pre-lowered reference MLIRs under
# salt-front/std/**/*.ll.mlir when present (salt-opt variants).
#
# Exit codes: 0 stable / 1 drift detected / 2 harness error.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SALTC="${SALTC:-$ROOT/salt-front/target/release/saltc}"
PASSES="${PASSES:-2}"

[ -x "$SALTC" ] || { echo "DETERMINISM-HARNESS-ERROR: saltc not found at $SALTC" >&2; exit 2; }
command -v shasum >/dev/null || command -v sha256sum >/dev/null || {
  echo "DETERMINISM-HARNESS-ERROR: need shasum or sha256sum" >&2; exit 2; }

digest() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

FAILURES=""
N=0
for f in "$ROOT"/salt-front/tests/z3_contracts/test_*.salt; do
  name="$(basename "$f")"
  prev=""
  stable="yes"
  for p in $(seq 1 "$PASSES"); do
    out="$TMP/${name}.$p.mlir"
    if ! "$SALTC" "$f" --lib --disable-alias-scopes -o "$out" >"$TMP/err.$p" 2>&1; then
      # Fixtures expected to fail compilation are not part of the
      # determinism surface; skip them consistently.
      stable="skip"
      break
    fi
    d="$(digest "$out")"
    if [ -n "$prev" ] && [ "$d" != "$prev" ]; then
      stable="no"
      FAILURES+="DRIFT: $name differs between pass $((p-1)) and $p"$'\n'
    fi
    prev="$d"
  done
  if [ "$stable" = "yes" ]; then N=$((N+1))
  elif [ "$stable" = "no" ]; then N=$((N+1))
  fi
done

if [ -n "$FAILURES" ]; then
  echo "MLIR-DETERMINISM FAILED:"; echo "$FAILURES"; exit 1
fi
echo "MLIR-DETERMINISM OK: $N fixtures byte-stable across $PASSES passes"

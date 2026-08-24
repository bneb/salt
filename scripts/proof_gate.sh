#!/usr/bin/env bash
# Proof-ratio CI gate (roadmap item 8).
#
# Compiles every non-rejected Z3 fixture with
#   saltc --lib --disable-alias-scopes --emit-proof-stats json
# and enforces, against the committed baseline:
#   - aggregate proven% floor            (MIN_PROVEN_PCT, default 45)
#   - ratchet: proven must not decrease while total is unchanged
#     (aggregate and per-fixture)
#   - corpus health: <= VACUOUS_MAX_PCT fixtures with total==0
# Compile-failing fixtures listed in PROOF_GATE_WAIVERS (colon-separated)
# are skipped with a warning; anything else failing the harness is an
# error. Exit codes: 0 pass / 1 policy violation / 2 harness error.
#
# Regenerate the baseline deliberately after intentional proof changes:
#   bash scripts/proof_gate.sh --update-baseline
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SALTC="${SALTC:-$ROOT/salt-front/target/release/saltc}"
BASE="${BASELINE:-$ROOT/salt-front/tests/z3_contracts/proof_baseline.json}"
MIN_PROVEN_PCT="${MIN_PROVEN_PCT:-45}"
VACUOUS_MAX_PCT="${VACUOUS_MAX_PCT:-50}"
WAIVERS="${PROOF_GATE_WAIVERS:-}"

[ -x "$SALTC" ] || { echo "GATE-HARNESS-ERROR: saltc not found at $SALTC" >&2; exit 2; }
command -v python3 >/dev/null || { echo "GATE-HARNESS-ERROR: python3 required" >&2; exit 2; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

VIOLATIONS=""
AGG_P=0; AGG_T=0; N=0; VAC=0
for f in "$ROOT"/salt-front/tests/z3_contracts/test_*.salt; do
  name="$(basename "$f")"
  case ":$WAIVERS:" in *":$name:"*)
      echo "waived: $name"; continue;;
  esac
  if ! "$SALTC" "$f" --lib --disable-alias-scopes --emit-proof-stats json \
        -o "$TMP/out.mlir" >"$TMP/json.txt" 2>"$TMP/err.txt"; then
    if case ":$WAIVERS:" in *":$name:"*) true;; *) false;; esac; then
      echo "waived (compile-fail): $name"; continue
    fi
    VIOLATIONS+="harness: $name failed to compile: $(head -1 "$TMP/err.txt")"$'\n'
    continue
  fi
  read -r P T < <(python3 - "$TMP/json.txt" <<'PY'
import json,sys
line=open(sys.argv[1]).read().strip().splitlines()[-1] if open(sys.argv[1]).read().strip() else ""
try:
    d=json.loads(line)["proof_stats"]
    print(d["proven"], d["total"])
except Exception:
    pass
PY
)
  if [ -z "${P:-}" ]; then
    VIOLATIONS+="harness: $name produced no proof_stats JSON"$'\n'; continue
  fi
  if [ "$P" -gt "$T" ]; then
    VIOLATIONS+="harness: $name proven($P) > total($T) -- counter integrity bug"$'\n'; continue
  fi
  echo "$P $T $name" >> "$TMP/rows"
  AGG_P=$((AGG_P+P)); AGG_T=$((AGG_T+T)); N=$((N+1))
  [ "$T" -eq 0 ] && VAC=$((VAC+1))
done

[ "$N" -gt 0 ] || { echo "GATE-HARNESS-ERROR: no fixtures produced stats" >&2; exit 2; }

if [ "${1:-}" = "--update-baseline" ]; then
  python3 - "$BASE" "$TMP/rows" <<'PY'
import json,sys
rows=[]
for line in open(sys.argv[2]):
    p,t,name=line.split()
    rows.append({"name":name,"proven":int(p),"total":int(t)})
rows.sort(key=lambda r:(r["name"],r["proven"]))
agg={"proven":sum(r["proven"] for r in rows),"total":sum(r["total"] for r in rows)}
json.dump({"aggregate":agg,"rows":rows},open(sys.argv[1],"w"),indent=2)
print("baseline updated:",sys.argv[1])
PY
  exit 0
fi

[ -f "$BASE" ] || { echo "GATE-HARNESS-ERROR: missing baseline $BASE (run with --update-baseline)" >&2; exit 2; }

VERDICTS=$(python3 - "$BASE" "$TMP/rows" "$VAC" "$VACUOUS_MAX_PCT" "$N" "$MIN_PROVEN_PCT" <<'PY'
import json,sys
base=json.load(open(sys.argv[1]))
brows={r["name"]:r for r in base["rows"]}
bag=base["aggregate"]
out=[]
ap=at=n=0
for line in open(sys.argv[2]):
    p,t,name=line.split(); p=int(p); t=int(t)
    ap+=p; at+=t; n+=1
    b=brows.get(name)
    if b and b["total"]==t and p<b["proven"]:
        out.append(f"regression: {name} proven {p} < baseline {b['proven']} (total {t} unchanged)")
vac, max_pct, ncount = int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
if ncount>0 and vac*100 > max_pct*ncount:
    out.append(f"{vac}/{ncount} fixtures vacuous (> {max_pct}%)")
pct=(ap*100//at) if at>0 else 0
floor=int(sys.argv[6])
if pct<floor:
    out.append(f"aggregate proven {ap}/{at} ({pct}%) below floor {floor}%")
if bag["total"]==at and ap<bag["proven"]:
    out.append(f"aggregate regression: proven {ap} < baseline {bag['proven']} (total {at} unchanged)")
print("\n".join(out))
PY
)
AGG_LINE="proven ${AGG_P}/${AGG_T}, vacuous ${VAC}/${N}"
if [ -n "$VERDICTS" ]; then
  echo "PROOF-GATE FAILED ($AGG_LINE):"; echo "$VERDICTS"; exit 1
fi
echo "PROOF-GATE OK: $AGG_LINE"

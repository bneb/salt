#!/bin/bash
# =============================================================================
# timing_harness.sh — native compile-time benchmark for Salt programs
# =============================================================================
#
# Measures how long saltc takes to COMPILE a fixed set of representative
# .salt programs (wall clock), reports per-program mean/min/max, compares
# against a JSON baseline, and flags >threshold regressions. This tracks
# compiler performance over time; it does not run the compiled programs.
#
# Dependencies: bash + python3 only. Safe to run standalone or from CI.
#
# Usage:
#   bash scripts/timing_harness.sh                      # run + compare
#   bash scripts/timing_harness.sh --update-baseline    # re-record baseline
#   bash scripts/timing_harness.sh --runs 10 --json out.json
#
# Options:
#   --runs N            timed compiles per program      (default 5)
#   --warmup N          untimed warmup compiles         (default 2)
#   --timeout SECS      per-compile watchdog            (default 120)
#   --threshold PCT     regression threshold in percent (default 10)
#   --saltc PATH        compiler binary                 (default
#                       salt-front/target/release/saltc)
#   --fixtures DIR      fixture root                    (default
#                       benchmarks/fixtures)
#   --baseline FILE     baseline JSON                   (default
#                       benchmarks/baseline_timings.json)
#   --json FILE         also write the full run results JSON to FILE
#   --update-baseline   store this run as the new baseline
#   --require-baseline  fail (exit 2) when the baseline file is missing
#
# Exit codes: 0 = clean; 1 = regression or compile failure;
#             2 = usage/environment error.
# =============================================================================
set -u -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SALTC="$WS_ROOT/salt-front/target/release/saltc"
FIXTURES="$WS_ROOT/benchmarks/fixtures"
BASELINE="$WS_ROOT/benchmarks/baseline_timings.json"
RUNS=5 WARMUP=2 TIMEOUT_SECS=120 THRESHOLD=10
UPDATE=0 REQUIRE_BASELINE=0 JSON_OUT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --runs)             RUNS="$2";                shift 2 ;;
        --warmup)           WARMUP="$2";              shift 2 ;;
        --timeout)          TIMEOUT_SECS="$2";        shift 2 ;;
        --threshold)        THRESHOLD="$2";           shift 2 ;;
        --saltc)            SALTC="$2";               shift 2 ;;
        --fixtures)         FIXTURES="$2";            shift 2 ;;
        --baseline)         BASELINE="$2";            shift 2 ;;
        --json)             JSON_OUT="$2";            shift 2 ;;
        --update-baseline)  UPDATE=1;                 shift ;;
        --require-baseline) REQUIRE_BASELINE=1;       shift ;;
        -h|--help) sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "timing_harness: unknown arg: $1" >&2; exit 2 ;;
    esac
done

if [[ ! -x "$SALTC" ]]; then
    echo "timing_harness: compiler not executable: $SALTC" >&2
    echo "  build it with: cargo build --release (in salt-front/)" >&2
    exit 2
fi
if [[ ! -d "$FIXTURES" ]]; then
    echo "timing_harness: fixtures dir not found: $FIXTURES" >&2
    exit 2
fi
case "$RUNS$WARMUP$TIMEOUT_SECS$THRESHOLD" in
    *[!0-9.]*) echo "timing_harness: numeric option invalid" >&2; exit 2 ;;
esac
if (( UPDATE == 0 )) && (( REQUIRE_BASELINE == 1 )) && [[ ! -f "$BASELINE" ]]; then
    echo "timing_harness: baseline missing: $BASELINE" >&2
    exit 2
fi

TH_HARNESS_ROOT="$WS_ROOT" TH_SALTC="$SALTC" TH_FIXTURES="$FIXTURES" \
TH_BASELINE="$BASELINE" TH_RUNS="$RUNS" TH_WARMUP="$WARMUP" \
TH_TIMEOUT="$TIMEOUT_SECS" TH_THRESHOLD="$THRESHOLD" TH_UPDATE="$UPDATE" \
TH_JSON_OUT="$JSON_OUT" python3 <<'PYEOF'
import json, os, platform, shutil, socket, statistics, subprocess, sys
import tempfile, time

ROOT = os.environ["TH_HARNESS_ROOT"]
SALTC_ENV = os.environ["TH_SALTC"]
FIXTURES = os.path.abspath(os.environ["TH_FIXTURES"])
BASELINE = os.path.abspath(os.environ["TH_BASELINE"])
RUNS, WARMUP = int(os.environ["TH_RUNS"]), int(os.environ["TH_WARMUP"])
TIMEOUT = float(os.environ["TH_TIMEOUT"])
THRESHOLD = float(os.environ["TH_THRESHOLD"])
UPDATE, JSON_OUT = os.environ["TH_UPDATE"] == "1", os.environ["TH_JSON_OUT"]
FLAGS = ["--lib", "--disable-alias-scopes"]


def die(msg):
    print(f"timing_harness: {msg}", file=sys.stderr)
    sys.exit(2)


def discover(fixtures):
    """Return [(name, [source paths])] sorted by name."""
    programs = []
    for entry in sorted(os.listdir(fixtures)):
        path = os.path.join(fixtures, entry)
        if entry.startswith("."):
            continue
        if os.path.isfile(path) and entry.endswith(".salt"):
            programs.append((entry[:-5], [path]))
        elif os.path.isdir(path):
            members = sorted(
                os.path.join(path, f) for f in os.listdir(path)
                if f.endswith(".salt")
            )
            if members:
                main = os.path.join(path, "main.salt")
                if main in members:  # entry point compiles last
                    members.remove(main)
                    members.append(main)
                programs.append((entry, members))
    return sorted(programs)


def combine(members, scratch):
    """One translation unit: package + imports hoisted on top.

    saltc does not cross-link separate files (symbols are package-
    qualified), so multi-file programs must compile as one source.
    Import lines appearing after other items corrupt codegen, so every
    package/import line is hoisted above all bodies.
    """
    pkg, imports, bodies = None, [], []
    for member in members:
        body = [f"\n// ---- module: {os.path.basename(member)} ----\n"]
        for line in open(member, encoding="utf-8"):
            t = line.strip()
            if t.startswith("package "):
                pkg = pkg or t
            elif t.startswith(("import ", "use ")):
                if t not in imports:
                    imports.append(t)
            else:
                body.append(line)
        bodies.append("".join(body))
    header = "\n".join([pkg or "package bench.harness"] + imports) + "\n"
    combined = os.path.join(scratch, "combined.salt")
    with open(combined, "w", encoding="utf-8") as fh:
        fh.write(header + "".join(bodies))
    return combined


def one_compile(src, out_mlir):
    """Run saltc once; return (ok, seconds, stderr tail)."""
    cmd = [SALTC_ENV, src] + FLAGS + ["-o", out_mlir]
    started = time.monotonic()
    try:
        proc = subprocess.run(cmd, capture_output=True, timeout=TIMEOUT)
    except subprocess.TimeoutExpired:
        return False, time.monotonic() - started, "watchdog timeout"
    elapsed = time.monotonic() - started
    err = (proc.stderr or b"").decode(errors="replace")[-400:]
    if proc.returncode != 0:
        return False, elapsed, err.strip()
    return True, elapsed, ""


def measure(name, members, scratch):
    """Warm up, then collect RUNS timed samples for one program."""
    out_mlir = os.path.join(scratch, name + ".mlir")
    src = combine(members, scratch)
    for _ in range(WARMUP):
        ok, _, err = one_compile(src, out_mlir)
        if not ok:
            return {"status": "compile-failed", "error": err}
    samples = []
    for _ in range(RUNS):
        ok, secs, err = one_compile(src, out_mlir)
        if not ok:
            return {"status": "compile-failed", "error": err}
        samples.append(secs)
    return {
        "status": "ok",
        "mean_s": round(statistics.fmean(samples), 6),
        "min_s": round(min(samples), 6),
        "max_s": round(max(samples), 6),
        "median_s": round(statistics.median(samples), 6),
        "samples_s": [round(s, 6) for s in samples],
    }


def load_baseline():
    if not os.path.isfile(BASELINE):
        return None
    try:
        with open(BASELINE, encoding="utf-8") as fh:
            data = json.load(fh)
    except (OSError, ValueError) as exc:
        die(f"baseline unreadable: {BASELINE} ({exc})")
    usable = isinstance(data, dict) and isinstance(data.get("programs"), dict)
    return data if usable else None


def verdict(now, base_info, thr_pct):
    """(label, delta_pct) comparing current mean against the baseline."""
    if now.get("status") != "ok":
        return "COMPILE-FAILED", None
    bmean = base_info.get("mean_s")
    if bmean is None:
        return "new", None
    pct = (now["mean_s"] - bmean) / bmean * 100.0
    if pct > thr_pct:
        return "REGRESSION", pct
    if pct < -thr_pct:
        return "improved", pct
    return "ok", pct


saltc_version = subprocess.run([SALTC_ENV, "--version"],
                               capture_output=True).stdout.decode().strip()


def run_all(programs, scratch):
    results = {}
    for name, members in programs:
        info = measure(name, members, scratch)
        info["sources"] = [os.path.relpath(m, ROOT) for m in members]
        info["source_bytes"] = sum(os.path.getsize(m) for m in members)
        results[name] = info
    return results


def print_report(results, base, thr_pct):
    print(f"salt compile-timing harness  saltc={os.path.relpath(SALTC_ENV, ROOT)}"
          f" ({saltc_version})")
    print(f"runs={RUNS} warmup={WARMUP} watchdog={TIMEOUT:g}s "
          f"threshold={thr_pct:g}%  fixtures={os.path.relpath(FIXTURES, ROOT)}")
    if base:
        where = os.path.relpath(BASELINE, ROOT)
        print(f"baseline: {where} (recorded {base.get('created_utc', '?')})")
    else:
        print("baseline: none (first run; use --update-baseline to record)")
    print()
    head = (f"{'program':<22} {'mean':>9} {'min':>9} {'max':>9}"
            f" {'base':>9} {'delta':>8}  verdict")
    print(head)
    print("-" * len(head))
    for name, info in results.items():
        base_info = (base or {}).get("programs", {}).get(name, {})
        label, pct = verdict(info, base_info, thr_pct)
        mean = f"{info['mean_s']:9.4f}" if info.get("status") == "ok" \
            else " " * 6 + "n/a"
        row = (f"{name:<22} {mean} {info.get('min_s', 0):9.4f}"
               f" {info.get('max_s', 0):9.4f}")
        if base_info.get("mean_s") is not None:
            row += f" {base_info['mean_s']:9.4f} {pct:+7.1f}%"
        else:
            row += f" {'—':>9} {'—':>7}"
        print(f"{row}  {label}")
        if info.get("status") == "ok" and info["max_s"] > 5 * info["min_s"]:
            print("    note: noisy samples (max > 5x min); "
                  "consider more runs on a quieter machine")


def summarize(results):
    """Return (#programs, regressions, [(name, error)])."""
    base = load_baseline()
    thr_pct = float((base or {}).get("regression_threshold_pct", THRESHOLD))
    print_report(results["programs"], base, thr_pct)
    print()
    failures, regressions = [], []
    for name, info in results["programs"].items():
        base_info = (base or {}).get("programs", {}).get(name, {})
        label, _ = verdict(info, base_info, thr_pct)
        if info.get("status") != "ok":
            err = info.get("error", "").splitlines()
            failures.append((name, err[-1] if err else "?"))
        elif label == "REGRESSION":
            regressions.append(name)
    for name, err in failures:
        print(f"COMPILE FAILED: {name}: {err}")
    return len(results["programs"]), regressions, failures


if RUNS < 1:
    die("--runs must be >= 1")
programs = discover(FIXTURES)
if not programs:
    die(f"no .salt fixtures found under {FIXTURES}")

results = {"schema": 1, "kind": "salt-compile-timing",
           "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
           "hostname": socket.gethostname(), "platform": platform.platform(),
           "saltc": os.path.relpath(SALTC_ENV, ROOT),
           "saltc_version": saltc_version,
           "runs": RUNS, "warmup": WARMUP, "timeout_secs": TIMEOUT,
           "regression_threshold_pct": THRESHOLD, "programs": {}}

scratch = tempfile.mkdtemp(prefix="salt-timing-")
try:
    results["programs"] = run_all(programs, scratch)
finally:
    shutil.rmtree(scratch, ignore_errors=True)

total, regressions, failures = summarize(results)

if UPDATE:
    results["kind"] = "salt-compile-timing-baseline"
    results["created_utc"] = results["generated_utc"]
    with open(BASELINE, "w", encoding="utf-8") as fh:
        json.dump(results, fh, indent=2)
        fh.write("\n")
    print(f"baseline written: {os.path.relpath(BASELINE, ROOT)}")

if JSON_OUT:
    with open(JSON_OUT, "w", encoding="utf-8") as fh:
        json.dump(results, fh, indent=2)
        fh.write("\n")
    print(f"results written: {JSON_OUT}")

print(f"\n{total} program(s): {len(regressions)} regression(s), "
      f"{len(failures)} compile failure(s)")
sys.exit(1 if (regressions or failures) else 0)
PYEOF

# Salt Benchmark Analysis

Source of truth for what the Salt benchmark suite measures, where the numbers
come from, and how to reproduce every figure in this document.

## 1. Methodology

Upstream methodology (`salt-benchmarks`, RESULTS.md / README.md), unchanged here:

- Hardware: Apple M4 Pro.
- C baseline: `clang -O3` (linked with `-lm`).
- Rust baseline: `rustc -O` per the README wording; the committed harness script
  compiles with `rustc -C opt-level=3` (equivalent optimization level).
- Timing: each binary is executed 2 times as warmup, then 5 measured runs under
  `/usr/bin/time -p`; the reported figure is the **median of the 5 runs**.
- Salt column: contract verification only (see section 4).

### Local-harness deltas

The fixed harness in this repository (`benchmarks/harness/bench.sh`,
`benchmarks/harness/check.sh`) keeps the methodology above and changes only
robustness and reporting:

1. **No silent aborts.** Upstream ran under `set -euo pipefail` with an unguarded
   timing pipeline. Any benchmark process that exited non-zero made
   `/usr/bin/time -p` propagate that status, `pipefail` failed the whole
   `time | grep | awk` pipeline, and `set -e` killed the entire run without a
   single diagnostic line. The local harness drops bare `set -e` and prints one
   visible `[warn] ...` line per failure instead, then continues.
2. **Per-run watchdog (`TIMEOUT_SECS`, default 10 s).** Upstream had no timeout,
   so any program that never exits blocked the harness forever. This is not
   hypothetical: `problems/echo` is an infinite TCP echo server and wedged the
   original harness at its warmup step (see section 6).
3. **True median-of-RUNS.** Upstream summed the samples and divided by RUNS —
   a mean — although RESULTS.md called it a median. The local harness sorts the
   valid samples and reports the actual median (mean of the two middle values
   for an even count).
4. **Parameterized paths.** `PROBLEMS_DIR` (any problems tree),
   `BUILD_OUT` (default `.round1-staging/build`, refused outside this workspace)
   and `SALTC` (default: the release-binary snapshot used for all figures here).
   Compile errors are kept in log files under `BUILD_OUT` and referenced from
   the warnings instead of being discarded to `/dev/null`.
5. **Degraded-sample honesty.** If fewer than RUNS samples succeed, the median
   is computed over the valid ones and the warning says exactly how many were
   usable. A column is reported as ✗ only when no sample succeeded at all.

## 2. Results

All upstream rows below are copied verbatim from the committed
`salt-benchmarks/RESULTS.md`. Rows marked *measured* were produced on
2026-08-23 by the local fixed harness against the same problem definitions,
using the snapshot binary `.round1-staging/saltc-snapshot` (saltc 1.2.0).

| Problem | C (s) | Rust (s) | Salt | Notes | Source |
|---------|-------|----------|------|-------|--------|
| fib | 0.162 | 0.172 | ✓ | Recursive fib(40) | upstream committed |
| sieve | 0.128 | 0.128 | ✓ | 10M primes | upstream committed |
| lru-cache | 0.010 | 0.010 | ✓ | LC 146 | upstream committed |
| sudoku-solver | 0.010 | 0.010 | ✓ | LC 37 | upstream committed |
| merge-sorted-lists | 0.010 | 0.010 | ✓ | LC 23 | upstream committed |
| trapping-rain-water | 0.060 | 0.060 | ✓ | LC 42 | upstream committed |
| longest-consecutive | 0.764 | 0.296 | ✓ | LC 128 | upstream committed |
| trie | 0.022 | 0.022 | ✓ | LC 208 | upstream committed |
| matmul | — | 0.132 | ✓ | 512×512 f64 | upstream committed |
| fannkuch | 0.130 | 0.130 | ✓ | n=11 | upstream committed |
| forest | 0.010 | 0.010 | ✓ | Union-find, 1000 ops | upstream committed |
| bitwise | 0.012 | 0.012 | ✓ | 10M ops | upstream committed |
| hashmap-bench | 0.012 | 0.010 | ✓ | 10K insert/lookup | upstream committed |
| vector-add | 0.092 | 0.090 | ✓ | 10M f32 | upstream committed |
| global-counter | 0.070 | 0.072 | ✓ | 10M atomic inc | upstream committed |
| http-parser-bench | 0.010 | 0.040 | ✓ | 10K requests | upstream committed |
| fstring-perf | 1.104 | 0.710 | ✓ | 100K format ops | upstream committed |
| buffered-writer-perf | 0.322 | 0.030 | ✓ | 100K writes, 8KB buf | upstream committed |
| writer-perf | 0.110 | 0.054 | ✓ | 100K writes | upstream committed |
| dll-salt | — | 0.020 | ✓ | No C baseline (C++ only) | upstream committed |
| window-access | 0.060 | 0.070 | ✓ | 10M indexed reads | upstream committed |
| string-hashmap-bench | 0.0200 | 0.0100 | ✓ | String-keyed hash map ops | measured 2026-08-23 via local harness |
| echo | — | — | ✓ | TCP echo server loop | unmeasured — upstream C/Rust sources do not build into runnable timing baselines (section 5); only Salt verification status, checked 2026-08-23 via local harness |
| two-sum | 0.0600 | 0.0600 | ✓ | LC 1, two-pointer, 200K elems x 500 queries | local port (benchmarks/problems/two-sum), measured 2026-08-23 |
| maximum-subarray | 0.1500 | 0.1500 | ✓ | LC 53, Kadane, 1M elems x 200 rounds | local port (benchmarks/problems/maximum-subarray), measured 2026-08-23 |
| valid-parentheses | 0.0900 | 0.0900 | ✓ | LC 20, explicit stack, 20K strings x len 4096 | local port (benchmarks/problems/valid-parentheses), measured 2026-08-23 |

Notes on precision: upstream committed values carry the precision shown in
RESULTS.md; locally measured values carry four decimals because the local
harness formats medians uniformly. Every locally measured figure sits at or
below the ~10 ms granularity of `/usr/bin/time -p` wall-clock reporting, so
they should be read as order-of-magnitude figures, not precise ratios.

Cross-validation: an independent measurement of string-hashmap-bench by the
second reviewer, same day and machine, reproduced C=0.020 / R=0.010 — matching
these medians exactly at display precision. There is no inter-run variance to
reconcile; the two measurements agree.

## 3. Where C wins, where Rust wins, why

Among the 19 upstream problems where both C and Rust compiled, the committed
numbers give: C faster on 3, Rust faster on 6, tied or within timer resolution
on the remaining 10 — nine exact ties at displayed precision plus
window-access; vector-add at 0.092 vs 0.090 counts as a marginal Rust win,
which is how upstream reached 3 + 6 + 10 = 19. Adding string-hashmap-bench
(a Rust win) and the three locally ported problems (each an exact tie at
displayed precision) yields 23 comparable rows: C faster on 3, Rust faster on
7, tied or within resolution on 13. All three ports are memory-scan/validation
kernels over arena-backed buffers, where both toolchains emit near-identical
tight loops.

- **C wins (3): fib, global-counter, http-parser-bench.** These are dominated by
  tight integer recursion (fib(40)), a raw atomic-increment loop, and a
  hand-written parser state machine respectively — code shapes where clang's
  backend emits essentially optimal machine code and there is no library
  boundary for Rust's standard library to amortize. The global-counter margin
  (0.070 vs 0.072) is within timer noise but was recorded as a C win upstream.
- **Rust wins (7): longest-consecutive, hashmap-bench, vector-add, fstring-perf,
  buffered-writer-perf, writer-perf, string-hashmap-bench.** The pattern across
  these is consistent: workloads that lean on formatting, buffered I/O, or
  hashing benefit from Rust's standard library avoiding printf-family overhead
  and shipping a modern hash table (hashbrown-style SIMD probing). The largest
  margins are exactly there: buffered writes (0.322 vs 0.030, ~10x) and string
  formatting (1.104 vs 0.710). For string-hashmap-bench both implementations do
  identical work (insert 1000 FNV-hashed string keys, look them up, remove half,
  re-insert, re-check — ×100 iterations); the hand-rolled C open-addressing table
  loses to Rust's std HashMap largely because the C side pays libc
  `snprintf` per key construction while Rust's `format!` path is cheaper, and
  the probe sequence is simpler. At 10 ms timer granularity the honest summary
  is: same ballpark, Rust measurably ahead.
- **Tied or within resolution (13): sieve, lru-cache, sudoku-solver,
  merge-sorted-lists, trapping-rain-water, trie, fannkuch, forest, bitwise,
  window-access (folded in, as upstream's own counts did), two-sum,
  maximum-subarray, valid-parentheses.**
  These kernels are memory-bound or pure arithmetic loops where clang and rustc
  converge on nearly identical machine code; differences land inside the timer's
  resolution. Upstream's summary line ("tied on 10") counted one of the
  near-ties (window-access at 0.060 vs 0.070, or vector-add at 0.092 vs 0.090)
  as equal; at face value the exact-tie count among upstream rows at displayed
  precision is 9, and all three local ports tie exactly.

The overall shape: neither language dominates. Workload class predicts the
winner — parser/loop kernels favor C's codegen, library-heavy work (formatting,
I/O, hashing) favors Rust's std, and everything else ties.

## 4. What the Salt column means today

The Salt column is **not** a native execution time. It records whether the Salt
compiler's Z3 contract checker verified the implementation:

```
saltc <problem>.salt --lib --disable-alias-scopes -o /dev/null
```

A ✓ means every public function's contracts — bounds checks, division safety,
loop invariants — were discharged at compile time. Native Salt timing requires
the LLVM 21 + `saltc --binary` pipeline, which is not part of either harness
yet, so no apples-to-apples Salt-vs-C/Rust timing claim can be made from these
tables.

## 5. Excluded from timing

- **keuos-train** — ML training kernel tied to the KeuOS kernel environment; no
  comparable standalone C/Rust driver exists in the suite.
- **bench-ecs-\*** (epoch-reclaim, event-pipeline, ipc-resolve, lookup,
  scheduler, spawn) — six ECS/kernel-specific micro-benches whose C/Rust
  counterparts depend on kernel-side infrastructure absent here; including them
  would compare different programs, not different languages.
- **syntactic-chaos** — a compiler stress test by design; it has no meaningful
  runtime phase to time.
- Shown with dashes in UPSTREAM RESULTS.md (not rows of the section 2 table
  above): binary-trees, binary-tree-path, chase-lev-bench, sliding-window-bench
  (their C and Rust sources need an arena header from the original project),
  trapping-rain-water-u64 (variant without independent baselines), and
  dll-salt's C column (C++ only upstream).
- **echo** — added by this measurement round but **unmeasured: its upstream
  baseline sources do not compile into runnable timing programs.** The Rust side
  fails under bare rustc with E0670 ("`async fn` is not permitted in Rust 2015"
  at `async fn main` — the file declares no edition, and rustc defaults to 2015)
  plus an `async move` edition error and E0433 (crate `tokio` unresolved;
  building it needs a cargo project). The C side compiles cleanly with clang but
  never terminates — it is a kqueue server parked in `kevent()`, which is also
  what wedged the upstream harness (section 6). With no finite C or Rust
  baseline available today, only the Salt contract-verification status is
  recorded; no timing numbers are invented for either column.

## 6. Case study: the silent-death bug in the upstream bench.sh

Running upstream `bash harness/bench.sh echo string-hashmap-bench` printed the
header and the padded name `echo` and then produced nothing — no columns, no
error, silent termination. Diagnosis, reproduced piece by piece locally:

1. **Primary cause — unbounded warmup.** `echo_c.c` is a kqueue TCP echo server
   whose main loop is `while (1)`. Upstream's warmup line
   (`"$out" >/dev/null 2>&1 || true`) guards against a non-zero exit status but
   not against blocking: once the server binds :8080 it sits in `kevent()`
   forever, and the harness hangs before the first timed run. On macOS the bind
   succeeds even with another listener on :8080 (SO_REUSEPORT semantics), so
   there is no fail-fast escape. With no timeout anywhere in the script, the run
   wedges after printing the padded name; killing it looks exactly like a silent
   exit. Reproduced verbatim: header + padded name printed, then nothing, until
   an external watchdog killed it.
2. **Latent cause A — pipefail + errexit data loss.** When a timed binary does
   exit non-zero, `/usr/bin/time -p` propagates that status (verified directly),
   `set -o pipefail` fails the whole `... | grep real | awk ...` pipeline even
   though grep and awk succeed and capture the correct seconds value, and the
   unguarded assignment trips `set -e`. Called bare, this kills the entire
   script instantly with zero output (reproduced with exit-code propagation);
   through the upstream call site's `$( )` subshell, macOS bash 3.2 does not
   apply errexit inside command substitutions, so the failure silently dissolves
   into an empty result rendered as ✗. Either way the operator sees no reason.
3. **Latent cause B — bc swallows emptiness.** `echo "" | bc -l` exits 0 and
   prints nothing, so an empty or malformed expression reaching the final
   `"scale=4; $total / $runs" | bc` yields an empty column rather than an error.
   In practice upstream's accumulator always stayed well-formed, so this remained
   latent rather than being the trigger.
4. **grep real under pipefail** was exonerated: grep is never the failing stage;
   the poison is stage one (the timed process's own exit status).

The local harness fixes all three layers: visible warnings instead of silent
degradation, a group-killing watchdog around every execution, and guarded
sample parsing that reports exactly which run failed and why.

## 7. Reproducing every number

Snapshot the compiler first (all figures here use the snapshot, never a moving
build):

```bash
mkdir -p .round1-staging && \
  cp salt-front/target/release/saltc .round1-staging/saltc-snapshot && \
  chmod +x .round1-staging/saltc-snapshot
```

Contract verification for all 36 problems (36 passed, 0 failed, 0 skipped on
2026-08-23):

```bash
PROBLEMS_DIR=/Users/kevin/projects/salt-benchmarks/problems \
  bash benchmarks/harness/check.sh
# default PROBLEMS_DIR covers the local tree:
bash benchmarks/harness/check.sh
```

The two newly measured rows:

```bash
PROBLEMS_DIR=/Users/kevin/projects/salt-benchmarks/problems \
  bash benchmarks/harness/bench.sh echo string-hashmap-bench
# -> string-hashmap-bench  C=0.0200s  R=0.0100s  S-check   (median of 5 after 2 warmups)
# -> echo                  no C/R columns, S-check   (unmeasured: broken upstream sources; loud [warn] lines explain each failure)
```

The three locally ported problems (default PROBLEMS_DIR is the local tree):

```bash
bash benchmarks/harness/check.sh
# -> 3 passed, 0 failed, 0 skipped (two-sum, maximum-subarray, valid-parentheses)
bash benchmarks/harness/bench.sh two-sum maximum-subarray valid-parentheses
# -> two-sum             C=0.0600s R=0.0600s S-check
# -> maximum-subarray    C=0.1500s R=0.1500s S-check
# -> valid-parentheses   C=0.0900s R=0.0900s S-check   (all medians of 5 after 2 warmups)
```

Exit-code contract: bench.sh always exits 0 - failures degrade to visible
`[warn]` lines so one broken problem cannot hide the rest of a run; check.sh
exits 1 on any FAIL.

Knobs: `RUNS=5 WARMUP=2 TIMEOUT_SECS=10 CC=clang RUSTC=rustc BUILD_OUT=...
SALTC=...` are all environment-overridable; defaults keep every write inside
this workspace.

Upstream committed numbers (section 2, "upstream committed" rows) were produced
in the reference checkout with its own harness:

```bash
cd /Users/kevin/projects/salt-benchmarks && make bench    # or: bash harness/bench.sh
cd /Users/kevin/projects/salt-benchmarks && make check    # or: bash harness/check.sh
```

Note that the upstream bench script still carries the defects in section 6;
running it over echo will wedge until killed.

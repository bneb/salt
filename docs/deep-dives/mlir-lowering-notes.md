# MLIR Lowering: Ground Truth Notes

> Stream W2 (lowering), Round 1. Everything below was verified by running code
> on this machine, not by reading docs. Where docs and code disagree, this file
> reports what actually happened.

## 1. The actual text pipeline

What exists today, as wired in code:

```
                    salt-front (Rust)                     external LLVM tools
 .salt -> parser -> typecheck/Z3 -> MLIR emitter -> textual .mlir
                                   codegen/mod.rs    |
                        SaltFile::emit_mlir          |  consumed by ONE of:
                                         |           +- scripts/run_test.sh (zsh)
                                         |           |     sed f32 fix
                                         |           |     mlir-opt@21  (15 distinct passes, 17 invocations)
                                         |           |     sed delete "salt.verify" lines
                                         |           |     mlir-translate@21 --mlir-to-llvmir
                                         |           |     sed weak_odr/global patches
                                         |           |     llvm-link@21 (deps)
                                         |           |     clang@21 -O3 + runtime.c + bridges
                                         |           |     native binary   (link FAILS today*)
                                         |           |
                                         |           +- saltc --binary / -c (driver.rs)
                                         |                 hardcoded llvm@18 paths
                                         |                 mlir-opt(12 passes) -> translate
                                         |                 -> llc -O3 (-reserved-reg keuos)
                                         |                 -> clang link        (FAILS today**)
                                         |
                                         +- default: writes out.mlir, stops. Dead end unless
                                            a script picks it up.
```

* run_test.sh fails at the final clang link: it unconditionally links
`user/os/facet_os.c`, `tests/bridges/ipc_bridge.c` and
`vendor/openlibm/libopenlibm.a` -- but **the user/ and vendor/ directories do
not exist in this checkout**; llvm@21 clang also defaults to a non-existent
sysroot (`/Library/Developer/CommandLineTools/SDKs/MacOSX26.sdk`).

** driver.rs fails at its final link with undefined symbols (`_start`,
`_write`, `_syscall`, `_waitpid`, ...): it links freestanding ("no libc") but
provides no start object or libc stubs for the macOS target.

### The one path that DOES work end-to-end (proved today)

The tool sequence itself is sound. Running the stages manually with two fixes
(skip missing bridges; explicit -isysroot):

```
saltc examples/fibonacci.salt -o fib.mlir                            # OK
mlir-opt@21 fib.mlir <run_test.sh pass list> -o fib.opt.mlir         # OK
mlir-translate@21 --mlir-to-llvmir fib.opt.mlir -o fib.ll            # OK
clang@21 -O3 -isysroot MacOSX.sdk fib_merged.ll runtime.c -o fib     # OK
./fib        # prints fib(40) correct, exit 0
```

So Salt -> textual MLIR -> LLVM IR -> optimized arm64 binary that runs and
gives the right answer **is achievable on this machine right now** -- but only
by hand; neither packaged path completes without intervention.

## 2. Who consumes emitted MLIR / out.mlir

| Consumer | Entry | Status |
|---|---|---|
| scripts/run_test.sh (zsh) | full pipeline + run | breaks at link (missing bridges/sysroot) |
| saltc --binary / -c (driver.rs) | in-process driver | breaks at link (undef symbols); hardcodes llvm@18 |
| tools/salt-build | delegates to scripts/run_test.sh | inherits its breakage |
| salt-opt/ C++ binary | nothing invokes it | not part of any wired path |
| src/qemu_build/sip_app.mlir | artifact | 0 bytes - stale committed junk |

On-disk artifacts `salt-front/out.mlir` and `salt-front/src/out.mlir` are (untracked; the repo's *.mlir gitignore rule excludes them)
outputs of past runs, not inputs to anything. The default CLI path writes
out.mlir (cli.rs) and stops; nothing downstream reads that exact filename.

## 3. Proven vs aspirational

| Claim (README/ARCH/ADR) | Verdict | Evidence |
|---|---|---|
| Salt compiles through MLIR to LLVM IR | **PROVEN** (by hand) | manual stage-by-stage run above; every stage exit 0 |
| "matching clang -O3" performance | **ASPIRATIONAL here** | no benchmark corpus or harness in this repo; numbers live in an external repo (bneb/salt-benchmarks). Linked binaries are post-processed with regex seds (weak_odr patching), so they are not plain clang output anyway. Nothing in-tree can reproduce or falsify the claim. |
| Multi-dialect emission via loop-body analysis (ARCH 2, ADR 003) | **PARTLY proven** | fibonacci emits ZERO affine/memref/vector ops (58 llvm, 36 arith, 28 func, 6 scf, 2 cf). affine+linalg+memref appear only for tensor-style programs (matmul_affine.salt: 3 affine, 3 linalg, 9 memref). Most programs bypass the polyhedral story entirely. |
| ARCH prereq LLVM 21+ vs driver.rs | **contradiction** | driver.rs hardcodes /opt/homebrew/opt/llvm@18/bin; run_test.sh defaults to @21; ADR 003 says tied to LLVM 21. |

More doc-vs-reality notes:

- The ARCH pass table matches neither run_test.sh nor driver.rs exactly;
  three divergent pass lists exist (e.g. driver.rs has --convert-vector-to-scf,
  run_test.sh does not).
- References to salt-front/tests/unit/*.salt: that directory does not exist.
  The real per-program corpus lives in salt-front/tests/cases/.
- contracts.salt still emits @__salt_contract_violation plus an scf guard,
  consistent with the documented Z3-UNKNOWN fallback, not with total erasure.

## 4. salt-opt build outcome (verbatim)

Attempt 1 - existing checked-in cache (salt-opt/build, configured against
llvm@18): configure OK, compile FAILED.

First failing command: make compiling CMakeFiles/salt-opt.dir/src/main.cpp.o
(errors began even earlier in src/passes/LowerSalt.cpp):

```
src/passes/LowerSalt.cpp:80:43: error: cannot initialize a parameter of type
  'Region *' with an rvalue of type 'Block *'
src/main.cpp:102:30: error: no viable conversion from 'llvm::Triple' to 'StringRef'
src/main.cpp:188:54: error: use of undeclared identifier 'bufferizationOpts'
src/main.cpp:208:20: error: no member named 'createSCFToControlFlowPass' in namespace 'mlir'
8 errors generated.
make[2]: *** [CMakeFiles/salt-opt.dir/src/main.cpp.o] Error 1
BUILD_EXIT=2
```

Attempt 2 - fresh configure against llvm@21 (matches ADR 003's stated
coupling):

```
cmake -S . -B .round1-staging/build21 \
  -DLLVM_DIR=/opt/homebrew/opt/llvm@21/lib/cmake/llvm \
  -DMLIR_DIR=/opt/homebrew/opt/llvm@21/lib/cmake/mlir
CONFIGURE21_EXIT=0
[100%] Built target salt-opt
BUILD21_EXIT=0
```

The C++ sources are fine against LLVM 21.1.8; the stale @18 cache was the
build blocker.

Runtime behavior of the fresh binary on real inputs:

- fibonacci.mlir (func/arith/scf/cf/llvm): --emit-llvm succeeds, 102-line .ll.
- matmul_affine.mlir: translation fails verbatim:

```
error: cannot be converted to LLVM IR: missing `LLVMTranslationDialectInterface`
registration for dialect for op: affine.for
LLVM Translation failed.
```

Root cause: buildLoweringPipeline gates the tensor/linalg path on detecting
tensor/linalg/bufferization/vector ops but never detects **affine**, and no
pass list contains --lower-affine; affine ops sail through untranslated.
Also: without --emit-obj/--emit-llvm, main() just parse+prints the module -
easy to mistake for a working lowering run when nothing was lowered.

## 5. Verification net (new file)

`scripts/verify_lowering.sh` (executable) compiles 8 programs through the
release saltc - 6 examples plus tests/cases/matmul_affine.salt and
tests/cases/regression_popcount.salt - and asserts per program:

(a) structure: module-attributes header, llvm.data_layout, llvm.target_triple,
    at least one function definition, balanced brace counts plus closing brace
    (brace balance catches head/mid/tail truncation; the last-line check alone
    missed tail-trims at brace boundaries until round-13 hardening);
(b) symbols: expected functions present (@main__fib, @main__safe_div,
    @__salt_contract_violation, arena runtime), per-program extras such as
    affine.for in matmul_affine and math.cttz dialect use in popcount;
(c) determinism: two fresh compilations byte-identical (cmp);
(d) any failure prints a targeted message + summary and exits 1; scratch dir
    from mktemp is removed unconditionally via explicit cleanup paths.

Observed tail of a green run:

```
PASS: fibonacci (examples/fibonacci.salt)
PASS: hello_world (examples/hello_world.salt)
PASS: pattern_matching (examples/pattern_matching.salt)
PASS: structs (examples/structs.salt)
PASS: contracts (examples/contracts.salt)
PASS: pipeline (examples/pipeline.salt)
PASS: matmul_affine (salt-front/tests/cases/matmul_affine.salt)
PASS: regression_popcount (salt-front/tests/cases/regression_popcount.salt)

8 passed, 0 failed
```

Corruption detection proof (staged copies under .round1-staging/w2-corruption/):

- truncated copy (500 bytes): FAIL ... no 'module attributes' header found.
- symbol rename (@main__fib -> @main__fib_gone, structure intact):
  FAIL ... expected symbol '@main__fib' not found.
- control: clean file passes all checks; self-test mode then correctly reports
  no corruption detected for it.

--corrupt-check FILE SYM[,SYM...] [REGEX] runs these assertions against any
existing .mlir, so CI can golden-check artifacts too.

## 6. Prioritized next REAL lowering tasks

1. P0 - fix salt-opt's affine hole: detect affine in buildLoweringPipeline's
   walk and insert createLowerAffinePass() (or affine->scf conversion) before
   SCF->CF. Regression test: matmul_affine.salt through saltc then salt-opt
   --emit-llvm must translate cleanly. Today it errors out (see section 4).
2. P0 - repair salt-opt build defaults: reconfigure the committed build dir
   against llvm@21 or drop it and document the two -D flags. The stale @18
   cache wastes everyone's first build attempt.
3. P1 - make run_test.sh's link stage self-contained: restore user/ and
   vendor/openlibm or gate those bridges behind existence checks; derive the
   sysroot from xcrun --show-sdk-path instead of the hardcoded SDK path.
4. P1 - unbreak saltc --binary: probe LLVM versions instead of hardcoding @18
   in driver.rs; supply start object/libc stubs (or link libc) so the final
   link stops failing on _start/_write/_syscall/_waitpid.
5. P2 - converge the three lowering pass lists (ARCH table, run_test.sh,
   driver.rs) into one canonical, tested sequence.
6. P2 - retire the sed glue: salt.verify line-deletion and weak_odr regex
   patching are silent-corruption magnets; move to real MLIR attributes/pass
   options once salt-opt owns the lowering.
7. P3 - docs honesty pass: update ARCH prereqs, qualify the multi-dialect
   claim as tensor-path-only, delete stale artifacts (src/out.mlir,
   empty sip_app.mlir, *.bak), point readers at verify_lowering.sh.

## 7. Reproduction commands

```
scripts/verify_lowering.sh
scripts/verify_lowering.sh --corrupt-check \
  .round1-staging/w2-corruption/fibonacci.symbol_rename.mlir main,main__fib
cmake -S salt-opt -B .round1-staging/build21 \
  -DLLVM_DIR=/opt/homebrew/opt/llvm@21/lib/cmake/llvm \
  -DMLIR_DIR=/opt/homebrew/opt/llvm@21/lib/cmake/mlir && cmake --build .round1-staging/build21 -j
```

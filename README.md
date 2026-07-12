# Salt

A systems programming language with Z3-powered compile-time verification.

[![CI](https://github.com/bneb/salt/actions/workflows/ci.yml/badge.svg)](https://github.com/bneb/salt/actions/workflows/ci.yml)

```salt
pub fn safe_div(a: i32, b: i32) -> i32
    requires(b != 0)            // Z3 proves this at compile time
{ return a / b; }
```

**Zero-cost contracts.** `requires` and `ensures` clauses are checked at compile
time. Proven checks are elided from the binary. Unproven checks become runtime
assertions. The compiler reports the ratio after every build:

```
Z3: 8/8 checks proven (100%), 0 deferred to runtime
```

**Compile-time type-bound proofs.** Contracts implied by the type system are
proven automatically. `requires(x < 256)` on a `u8` parameter always holds —
the compiler knows `u8 ∈ [0, 255]` and elides the check.

## Install

```bash
brew install z3
# macOS with Homebrew Z3:
C_INCLUDE_PATH=/opt/homebrew/include LIBRARY_PATH=/opt/homebrew/lib \
  cargo install saltc --git https://github.com/bneb/salt
# Linux (pkg-config):
cargo install saltc --git https://github.com/bneb/salt
```

Or build from source:

```bash
git clone https://github.com/bneb/salt.git
cd salt/salt-front
cargo build --release
```

Prerequisites: Rust 1.80+, Z3 4.8+ (`brew install z3`). macOS ARM64 users
have a pre-configured `.cargo/config.toml`; other platforms may need
`C_INCLUDE_PATH` and `LIBRARY_PATH` set for Z3 headers/libs.

## Try It

```bash
echo 'package main
pub fn safe_div(a: i32, b: i32) -> i32
    requires(b != 0)
{ return a / b; }

pub fn main() -> i32 {
    return safe_div(100, 10);
}' > demo.salt
saltc demo.salt --lib -o /dev/null
```

The contract is satisfied — compilation succeeds. Now violate it:

```bash
sed -i '' 's/10/0/' demo.salt
saltc demo.salt --lib -o /dev/null
# VERIFICATION ERROR: could not prove '(not (= 0 0))'
# counterexample: b = 0
```

Every build prints proof coverage. See the [contract test suite](salt-front/tests/z3_contracts/run_tests.sh) for 44 end-to-end examples.

## What's Inside

| Directory | What |
|-----------|------|
| `salt-front/` | Rust compiler (parser, type checker, Z3 verifier, MLIR emitter) |
| `salt-front/std/` | Standard library (70+ modules) |
| `salt-opt/` | C++ optimizer backend (MLIR passes, dialect definitions) |
| `tools/salt-lsp/` | Language Server Protocol + VS Code extension |
| `tools/salt-wasm/` | WASM bridge for browser playground |
| `tools/sp/` | Salt package manager |
| `docs/` | Language spec, tutorial, ADRs, blog posts |
| `examples/` | Example Salt programs |
| `site/` | salt-lang.dev website |

## Verification

Salt embeds the Z3 SMT solver. Contracts are checked during normal
compilation. No separate prover, no special flags. Use `--deny-deferred`
to turn any unproven check into a hard error in CI.

44 regression tests cover all verification modes: integer bounds,
postconditions, forall/exists quantifiers, loop invariants, array frame
axioms, struct field bounds, cross-function chaining, bitvectors, string
operations, type-bound proofs, Slice bounds, method call contracts, and
for-loop induction variable tracking.

```bash
bash salt-front/tests/z3_contracts/run_tests.sh
```

See [`docs/deep-dives/z3-contracts.md`](docs/deep-dives/z3-contracts.md)
for the full capability reference.

## Performance

Salt compiles through MLIR to LLVM IR, matching `clang -O3` on
compute workloads. Benchmarks across 22 algorithm problems compare
C, Rust, and Salt. See [salt-benchmarks](https://github.com/bneb/salt-benchmarks)
for source, methodology, and raw data.

## License

MIT

## Ecosystem

Salt powers these projects:

| Project | Description |
|---------|-------------|
| [KeuOS](https://github.com/bneb/keuos) | Microkernel with Z3-verified safety invariants |
| [Basalt](https://github.com/bneb/basalt) | Llama 2 inference — 920 tok/s, Z3-verified kernels |
| [Lettuce](https://github.com/bneb/lettuce) | Redis-compatible server with Z3-proven buffer bounds |
| [Facet](https://github.com/bneb/facet) | GPU 2D compositor — Metal backend, matches C performance |
| [Salt Benchmarks](https://github.com/bneb/salt-benchmarks) | 36 leetcode-style problems: Salt vs C/Rust |

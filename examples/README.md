# 🧂 Salt Examples

Learn Salt by example — from "Hello, World!" to formal verification.

## Getting Started

```bash
# Build the compiler (one time; Z3 env vars needed on a cold macOS build)
export C_INCLUDE_PATH=/opt/homebrew/include LIBRARY_PATH=/opt/homebrew/lib
cd salt-front && cargo build --release && cd ..

# Compile an example: prints the Z3 proof summary for its contracts,
# writes MLIR to hello.mlir
./salt-front/target/release/saltc examples/hello_world.salt -o hello.mlir

# Native binaries go through the LLVM pipeline (mlir-opt -> mlir-translate ->
# clang). It is not turn-key yet - see docs/deep-dives/mlir-lowering-notes.md
# for the working manual sequence and current packaging gaps.
```

## Examples

| File | Concepts | Difficulty |
|------|----------|------------|
| [hello_world.salt](hello_world.salt) | Functions, `println`, program structure | ⭐ Beginner |
| [fibonacci.salt](fibonacci.salt) | Recursion, `if/else`, `i32` types | ⭐ Beginner |
| [pipeline.salt](pipeline.salt) | `\|>` pipe operator, f-strings, `for` loops | ⭐ Beginner |
| [structs.salt](structs.salt) | `struct`, `impl`, methods, `Ptr<T>`, linked lists | ⭐⭐ Intermediate |
| [pattern_matching.salt](pattern_matching.salt) | `enum`, `match`, `Result<T,E>`, error handling | ⭐⭐ Intermediate |
| [contracts.salt](contracts.salt) | `requires()`, Z3 verification, formal proofs | ⭐⭐⭐ Advanced |
| [http_server.salt](http_server.salt) | Networking, kqueue, `StringView`, zero-copy I/O | ⭐⭐⭐ Advanced |

## Next Steps

- **Basalt**: See [Basalt Llama 2 inference](https://github.com/bneb/basalt) for a ~600-line Llama 2 inference engine — a complete real-world Salt application with Z3-verified kernels
- **Benchmarks**: See [Salt Benchmarks](https://github.com/bneb/salt-benchmarks) and [docs/benchmarks/ANALYSIS.md](../docs/benchmarks/ANALYSIS.md) — C/Rust baselines timed across the suite, with per-number provenance
- **Standard Library**: See [`salt-front/std/`](../salt-front/std/) for the full module reference
- **Language Spec**: See [`docs/SPEC.md`](../docs/SPEC.md) for the formal specification

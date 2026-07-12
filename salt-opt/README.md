# Salt Backend (salt-opt)

**The Mission:** The C++ backend that verifies Salt contracts via Z3 before lowering to machine code.

## Overview
This is a custom MLIR-based compiler backend. It takes the high-level MLIR emitted by `salt-front`, verifies it, optimizes it, and emits an LLVM Object File.

## The compilation Pipeline

```mermaid
graph TD
    MLIR[High Level MLIR] --> Verify[Z3 Verification Pass]
    Verify --> Opt[Optimization (O3)]
    Opt --> Lower[Lowering to LLVM IR]
    Lower --> Obj[Emit Object File]
```

## Invariants

> [!IMPORTANT]
> **Verification First**
> The `Z3VerifyPass` runs *before* any potentially destructive optimization or lowering.
> We prove the code is safe while it is still semantically rich.

### 1. The Boolean Law
The backend enforces that all booleans are strict `i1` until the final LLVM lowering, where they become bytes if stored in memory.

### 2. Zero-Cost Abstraction
The `OneShotBufferizePass` ensures that high-level tensor operations are converted to efficient pointer arithmetic (MemRefs) without runtime bounds checking overhead (proven unnecessary by Z3).

## Components

| Directory | Role |
|-----------|------|
| [`src/`](./src) | **Source Code.** The C++ backend logic. |
| [`src/passes/`](./src/passes) | **Passes.** Z3 Verification and Lowering logic. |
| [`src/dialect/`](./src/dialect) | **Dialect.** Definition of the `!salt` IR operations. |

## Build
```bash
# Handled by Bazel
bazel build //:salt-opt
```

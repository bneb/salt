# Backend Source Layout

**The Mission:** House the MLIR passes and dialect definitions.

> **Note**: This is the legacy C++ backend. The primary compiler is [salt-front](../../salt-front/) (Rust). This backend is retained for dialect definitions, but Z3 verification and code generation now run in `salt-front`.

## Key Files

| File | Role | Mechanisms |
|------|------|------------|
| [`main.cpp`](./main.cpp) | **The Driver.** | Builds the PassManager pipeline. Configures LLVM targets. |
| [`passes/Z3Verify.cpp`](./passes/Z3Verify.cpp) | **The Judge (Legacy).** | Superseded by `salt-front`'s Z3 Proof-or-Panic: proven contracts are elided; unproven lower to `scf.if`. |
| [`passes/LowerSalt.cpp`](./passes/LowerSalt.cpp) | **The Lowerer.** | Converts `!salt.region` and other high-level constructs to LLVM pointers. |
| [`dialect/SaltOps.td`](./dialect/SaltOps.td) | **The IR.** | TableGen definitions for Salt operations. |

## The Optimization Pipeline (`main.cpp`)
1. **Canonicalize & CSE:** Cleanup output from frontend.
2. **Z3 Verify:** Prove invariants.
3. **LowerSalt:** Remove Salt abstractions.
4. **Bufferize:** Tensor → MemRef.
5. **SCFToControlFlow:** Loops → Branches.
6. **ToLLVM:** Final conversion to LLVM Dialect.

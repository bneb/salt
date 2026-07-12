# Salt Passes

**The Mission:** Transform, Verify, and Lower.

## Components

| File | Role | Invariant |
|------|------|-----------|
| [`Z3Verify.cpp`](./Z3Verify.cpp) | **Formal Verification.** | Must run *before* any lowering. Converts boolean logic to SMT2. |
| [`LowerSalt.cpp`](./LowerSalt.cpp) | **LLVM Lowering.** | Converts `!salt.region` to `!llvm.ptr`. |

## The Z3 Bridge (Legacy)

> **Note**: `salt-front` (the active Rust compiler) no longer emits `salt.verify` ops. Contracts are now handled via **Z3 Proof-or-Panic**: proven contracts are elided at compile time; unproven contracts lower to standard MLIR `scf.if` + `@__salt_contract_violation`.

The legacy `Z3VerifyPass` walks the MLIR CFG (Control Flow Graph).
1. **Extract:** Finds all `arith.cmpi` and `salt.verify` ops.
2. **Encode:** Translates them to Z3 C++ API calls.
3. **Solve:** If `solver.check() == unsat` (for the negation of the invariant), the code is safe.

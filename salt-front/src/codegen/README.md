# Frontend Code Generation

**The Mission:** Convert the typed Salt AST into MLIR.

## Overview

This module handles the translation of Salt surface syntax into MLIR dialects (`affine`, `scf`, `func`, `llvm`). It is responsible for type erasure, method resolution, generic monomorphization, and the hoisting of stack allocations.

The codegen runs in five phases (see [phases/](./phases/)):

```
Discovery → Expansion → Verification → Control Flow → Emission
```

## Architecture

### Core

| File | Role |
|------|------|
| [`mod.rs`](./mod.rs) | **Driver.** The `emit_mlir` entry point. Orchestrates all phases and passes. |
| [`context.rs`](./context.rs) | **Symbol Table.** Manages variable scope, type mapping, and SSA register allocation. |
| [`stmt.rs`](./stmt.rs) | **Statement Lowering.** Handles `if`, `while`, `let`, `for`, `match`. Enforces Alloca Hoisting Law. |
| [`module_loader.rs`](./module_loader.rs) | **Imports.** Resolves `use std.collections.HashMap` to stdlib paths. |

### Expressions ([`expr/`](./expr/))

| File | Role |
|------|------|
| [`mod.rs`](./expr/mod.rs) | Expression lowering driver — dispatches to specialized handlers. |
| [`resolver.rs`](./expr/resolver.rs) | Method resolution and UFCS (Uniform Function Call Syntax). |
| [`utils.rs`](./expr/utils.rs) | Shared helpers for expression emission. |
| [`aggregate_eq.rs`](./expr/aggregate_eq.rs) | Structural equality for structs and enums. |
| [`while_loop.rs`](./expr/while_loop.rs) | While loop → `scf.while` lowering with SSA iter_args. |
| [`tensor_ops.rs`](./expr/tensor_ops.rs) | Tensor operations → `affine.for` with polyhedral tiling. |

### Passes ([`passes/`](./passes/))

| File | Role |
|------|------|
| [`async_to_state.rs`](./passes/async_to_state.rs) | `@yielding` function transformation (Pulse concurrency). |
| [`loop_invariant.rs`](./passes/loop_invariant.rs) | Loop invariant inference for Z3 verification. |
| [`liveness.rs`](./passes/liveness.rs) | Variable liveness analysis for register allocation. |
| [`call_graph.rs`](./passes/call_graph.rs) | Inter-procedural call graph construction. |
| [`sync_verifier.rs`](./passes/sync_verifier.rs) | Thread safety verification (mutex/atomic analysis). |
| [`pulse_injection.rs`](./passes/pulse_injection.rs) | Preemption checkpoint injection for `@pulse(N)` loops. |

### Phases ([`phases/`](./phases/))

| Phase | File | Purpose |
|-------|------|---------|
| 1. **Discovery** | [`discovery.rs`](./phases/discovery.rs) | Collect all types, functions, traits. Build global symbol table. |
| 2. **Expansion** | [`expansion.rs`](./phases/expansion.rs) | Monomorphize generics, resolve trait impls. |
| 3. **Verification** | [`verification.rs`](./phases/verification.rs) | Run Z3 contract verification, arena escape analysis. |
| 4. **Control Flow** | [`control_flow.rs`](./phases/control_flow.rs) | Lower `match`/`if`/`while` to structured control flow. |
| 5. **Emission** | [`emission.rs`](./phases/emission.rs) | Emit final MLIR text output. |

### Other

| File | Role |
|------|------|
| [`intrinsics.rs`](./intrinsics.rs) | Built-in functions (`println`, `sizeof`, `ptr_read`, `ptr_write`). |
| [`generic_resolver.rs`](./generic_resolver.rs) | Multi-parameter generic resolution (`Vec<T, A>`). |
| [`struct_deriver.rs`](./struct_deriver.rs) | `@derive(Clone, Eq, Hash)` code generation. |
| [`const_eval.rs`](./const_eval.rs) | Compile-time constant evaluation. |
| [`abi.rs`](./abi.rs) | ABI-level struct layout and FFI bridge generation. |
| [`seeker.rs`](./seeker.rs) | Type/symbol lookup across module boundaries. |
| [`collector.rs`](./collector.rs) | Collects referenced types for forward declaration. |

# Compiler Source Layout

**The Mission:** Iterate from Source Text $\rightarrow$ AST $\rightarrow$ MLIR.

## The Pipeline

### 1. Parsing (`grammar/`)
Custom recursive-descent parser for Salt syntax (handles `|>`, `|?>`, `@`, `_` placeholder, and verification clauses).
- **Pulse Spec:** Loops can be decorated with `@pulse(N)` to control preemption checks.
- **Concepts:** First-class support for `concept <T> requires(...)`.

### 2. Analysis (`passes/`)
- **Type Resolution:** `type_bridge.rs` resolves Salt types to `Type::I32`, `Type::Struct`, etc.
- **Mutation Tracking:** `collect_mutations` pre-scans function bodies to decide which variables need stack slots.

### 3. Code Generation (`codegen/`)
- **Block Emission:** `emit_block` lowers statements to MLIR `cf.br` control flow.
- **Alloca Hoisting:** `hoist_allocas_in_block` moves all allocations to the top.

## Key Files

| File | Role | Invariant |
|------|------|-----------|
| [`main.rs`](./main.rs) | **Entry Config.** | CLI argument parsing. |
| [`codegen/stmt.rs`](./codegen/stmt.rs) | **Statement Lowering.** | **Hoisting:** `hoist_allocas_in_block`. |
| [`codegen/mod.rs`](./codegen/mod.rs) | **Orchestrator.** | **Pre-Scan:** `pre_scan_workspace`. |

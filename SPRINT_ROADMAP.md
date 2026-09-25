# Salt — Sprint Roadmap

*A systems programming language where memory safety is **proved**, not promised.*

This document turns that thesis into an ordered plan of work. It states where Salt
is going, what we accept about where it is today, and six sprints of concrete,
exit-criteria-driven work that closes the gap between the two.

---

## 1. North Star

Salt's bet is that the next systems language does not win by adding another
borrow checker. It wins by making **proof** the default compilation step:

> Every safety property a programmer writes down — `requires(b != 0)`,
> slice bounds, lifetimes of arena blocks, ownership transfers — is checked by
> the compiler with Z3 at build time. Checks that are proven vanish from the
> binary. Checks that cannot be proven become explicit runtime assertions, and
> the compiler says so. Nothing is silently unchecked.

The language we want, concretely:

- **Zero-cost by construction** — proofs remove code; they never add it.
- **Predictable degradation** — when a proof fails, the fallback is visible,
  counted (`Z3: 8/8 proven`), and actionable. Silence is a bug.
- **Determinism everywhere** — same input, same MLIR, byte-for-byte.
- **Ergonomics without escape hatches** — inference, trait defaults, preludes
  and f-strings so that writing verifiable code is the *easy* path.
- **Small core, verified stdlib** — `unsafe` in std is auditable line-by-line;
  every unsafe operation carries a `requires` clause.
- **Real targets** — kernels (KeuOS), daemons (netd), shaders (@shader). If it
  can't hold a kernel together, it isn't a systems language.

---

## 2. Where we are today (honest snapshot)

Evidence-based, post the trait-default-methods work (W-T/W-CM).

**Working and locked by tests:**
- Trait definitions parse with default bodies; implementors may omit them;
  explicit overrides win; inherited bodies monomorphize per impl type.
- Inheritance works across module boundaries, including default bodies that
  reference the trait's home-module types and call sites inside module code.
- Z3 contract pipeline proves bounds/preconditions via invariants and guards
  (44-case contract suite green); proof ratios reported per build.
- Monomorphization-on-demand hydration, enum ctor inference, method chains.

**Known debt (each traced to a file or failing probe):**
| # | Issue | Evidence |
|---|-------|----------|
| D1 | Field access on erased generic self-types fails: `[E003] Cannot access field v on Reference(Struct("main__Slot"))` — breaks *any* method body touching fields of `struct S<T>` | CLI probe, control-tested independent of defaults |
| D2 | Trait identity is bare-name based; registry lookup is first-match-wins over a legacy index; `register_trait_def` stores placeholder signatures | `src/codegen/trait_registry.rs` |
| D3 | Two sources of truth: codegen re-walks the grammar AST while HIR exists; HIR typeck is test-only | `src/codegen/mod.rs`, `src/hir/typeck.rs` |
| D4 | Dual impl-registration paths (loaded-file ASTs vs `ModuleInfo.impls` snapshots) created stale-snapshot hazards; now re-synced but order-dependent | pinned by `tests/trait_default_methods_cross_module_test.rs` |
| D5 | End-to-end integration tests need mlir-translate/clang and sit `#[ignore]`d | `tests/runner.rs` |
| D6 | ~30 pre-existing lint failures under `clippy --all-targets`; strict variant is not yet the gate | CI noise, verified CLEAN-at-HEAD |
| D7 | Dead/drifted artifacts: unused `emit_trait`, stale grammar doc comments | `src/codegen/mod.rs`, `src/grammar.rs` |

---

## 3. Working agreements (every sprint, non-negotiable)

| Gate | Bar |
|------|-----|
| `cargo test` | 0 failures; suite only grows (removed tests require an ADR note) |
| `bash tests/z3_contracts/run_tests.sh` | green; case count never decreases |
| `cargo clippy --all-targets -- -D warnings` | clean (adopted as the gate as of Sprint 1) |
| Style | ≤32 non-blank lines/function, ≤500/file, ≤3 nesting, zero TODO/FIXME markers |
| Proofs | no change may increase silent runtime-deferral of checks |
| VCS | nothing committed/pushed without explicit approval |

Every sprint ends with a short written retro appended to its section below
(one paragraph: what slipped, why, what we learned).

---

## 4. Sprint overview

Cadence: 2 weeks. Themes are sequential because each de-risks the next.

| Sprint | Theme | Kills |
|--------|-------|-------|
| S1 | Correctness foundations | D1, D6, D7 |
| S2 | One type system, one truth | D2, D3, D4 |
| S3 | Generics that earn trust | full generic traits + const generics |
| S4 | Verification as a product | proof UX, contract inheritance, std audit |
| S5 | Memory model consolidation | ownership/arena/temporal story |
| S6 | Toolchain & release engineering | D5, reproducibility, stdlib v1 |

---

### Sprint 1 — Correctness foundations

**Goal:** make the tree trustworthy enough to refactor against.

**Workstreams**
1. Fix D1 (generic field access on erased self-types). Start from the minimal
   repro in this doc; add regression tests for required *and* defaulted methods
   touching fields of generic structs.
2. Adopt `clippy --all-targets -- -D warnings` as the gate; burn down the
   existing ~30 failures file-by-file (mechanical, parallelizable).
3. Delete dead paths (`emit_trait`), fix drifted doc comments in grammar files,
   and add a lint/test that fails when public doc comments contradict behavior.

**Exit criteria**
- Generic struct field access works in impl method bodies (required + defaulted).
- `--all-targets` clippy is clean and mandatory in the working-agreements table.
- Zero dead-code warnings in `cargo build`.

**Retro (Sprint 1):** D1's root cause was struct-literal emission refusing to
infer generic arguments when the context type map was empty (top level), which
recorded locals as unspecialized templates with zero-field registry skeletons;
fixed in `codegen::expr::literals::emit_struct` by inferring from field
expressions and requiring full parameter arity before specializing. The lint
drive-through surfaced that several test files predate Result-returning
registration APIs; fixed mechanically. Concurrent-agent edit collisions hit
twice mid-sprint (context.rs, enum_ctor.rs) — integration-first policy held,
but file-ownership checks must happen per-edit, not per-session.

**Retro (Sprint 1, hydration lane):** probing isolated two further D1 causes
upstream of the literal fix. First, `specialize_template` cached the deferred
expansion result — an empty-fields stub — under the bare template name, so
every later lookup of the erased name found a struct with no fields; it now
detects that shape (`is_deferred_struct_expansion`) and leaves the entry
absent. Second, `hydrate_specialization` installed an erased generic self type
verbatim; it now adopts the unique concrete instance's arguments when exactly
one exists and fails loudly on ambiguity instead of guessing. Regression
tests for the erased `impl<T>` family (required + defaulted, both receivers,
generic returns) live in `tests/generic_impl_self_field_test.rs`; the doc-
behavior tripwire is `tests/doc_behavior_guard_test.rs`. Lesson: reproduce
through the CLI binary before reading code — the probe sequence turned a
three-site guessing game into two surgical fixes.

---

### Sprint 2 — One type system, one truth

**Goal:** trait/method identity stops being stringly and stops being duplicated.

**Workstreams**
1. Qualified trait identity: registry keys become `(package_path, trait_name)`;
   impl resolution resolves through name resolution results instead of bare
   idents. Same-named traits in different modules must coexist (test included).
2. Replace placeholder signatures in `register_trait_def` with real param/return
   types; retire first-match-wins in favor of deterministic overload resolution
   with a defined tie-break rule.
3. ADR: pick one canonical representation. Either (a) HIR becomes the method-
   dispatch source and codegen consumes HIR, or (b) AST stays canonical and HIR
   is promoted to a checked view — either way, write down the phase-ordering
   invariants (the cross-module tests become the executable form of this ADR).
4. Fold `ModuleInfo.impls` snapshot logic into that decision; delete the re-sync
   if (a)/(b) makes it redundant.

**Exit criteria**
- Name-collision test suite green; overload ties impossible by construction.
- ADR merged; exactly one registration path per impl in the chosen design.

---

### Sprint 3 — Generics that earn trust

**Goal:** generics graduate from inference tricks to a real system.

**Workstreams**
1. End-to-end generic trait impls: `impl<T> Prod for Slot<T>` with omitted
   defaults, default bodies referencing associated types of `T`, and calls
   through both concrete and still-generic contexts.
2. Const generics: the grammar already carries `GenericParam::Const`; wire it
   through lowering, monomorphization, and Z3 facts (const values as solver
   assumptions — this is where verification and generics compound).
3. Monomorphization hygiene: dedup identical instantiations, compile-time
   budget alarm (`monomorphization_stress` as the canary), p50/p95 tracked.


**Retro (federated sessions, rounds 7–20):** Sprint 3's "generics that earn
trust" thesis was stress-tested by an adversarial-probe campaign that found
and closed the entire ghost-identity family at its root — arity-tolerant
unification, prefix-tolerant numeric promotion, value-spelled-leaf guards,
move-transfer hooks for by-value resources into callees, and an actionable
[E003] for uninferrable type parameters. A typed identity scaffold
(`InstanceId`, refusing constructors) and a single registration chokepoint
(`define_instance`) landed unwired ahead of the S3-6 schema migration.
Mechanical enforcement grew a spelling-goldens gate (16+ pinned emission
contracts incl. stderr refusal rows) and artifact-absence checks. Open:
full local type inference (WS-7), instance-key arity normalization for
allocator placeholders (S3 final), naming-collision ICE (NB-1).

---
**Exit criteria**
- Generic-default matrix (required/defaulted × override/omit × generic/concrete
  receiver) fully green and committed as one test file.
- At least one std module using a const-generic parameter.

---

### Sprint 4 — Verification as a product

**Goal:** the prover stops being a backend detail and becomes the UX.

**Workstreams**
1. Proof status surface: per-function/per-module proven-vs-deferred report;
   `saltc build --proof-report` emitting JSON; CI threshold flags regressions.
2. Diagnostics that teach: for every deferred check, print *why* (missing
   invariant? unsupported construct?) and the exact suggested fix shape.
3. Contract inheritance for traits: `requires`/`ensures` on trait methods
   (defaults already carry them syntactically); overrides must satisfy-or-
   strengthen defaults' contracts; Z3 checks the obligation at the impl site.
4. Stdlib unsafe audit: every `unsafe` op in `std/**` annotated and proved or
   explicitly waived with justification in `docs/UNSAFE.md`.

**Exit criteria**
- Proof-ratio regression is a hard CI failure.
- Contract-inheritance suite: default-contract obligations enforced on 100% of
  override forms (override, omit, mixed across modules).
- std audit table in `docs/UNSAFE.md` complete with zero unwaived gaps.

---

### Sprint 5 — Memory model consolidation

**Goal:** one coherent story: regions (arenas), ownership (consume), time.

**Workstreams**
1. Spec the model: arenas for region allocation, linear `consume` parameters
   for transfer, temporal-safety proofs for use-after-free — write it as a
   philosophy doc with the existing tests as its semantics.
2. Promote EBR, provenance, and temporal-safety suites from experimental to
   gating status.
3. Alias-scope/noalias correctness: validate emitted scope domains survive
   `salt-opt` pipelines without unsound elision (fuzz + differential testing).

**Exit criteria**
- Memory-model doc merged; all three suites blocking; no known unsoundness.

---

### Sprint 6 — Toolchain & release engineering

**Goal:** the compiler earns "daily driver" and "reproducible".

**Workstreams**
1. Un-`#[ignore]` the toolchain integration harness in CI (mlir-translate +
   clang runners) with artifact caching; execute-and-compare for `tests/cases`.
2. Reproducibility gate: hash final MLIR per fixture; mismatch = failure.
3. Module cache/incremental hydration keyed on (module, deps, saltc version).
4. stdlib v1 freeze: API review of core/io/net surfaces, doc-comments on every
   public item, changelog started.

**Exit criteria**
- CI runs execute-and-verify end-to-end on Linux + macOS.
- Byte-stable MLIR across repeat builds on the same toolchain.
- stdlib v1 tagged with frozen APIs and full doc coverage.

---

## 5. Cross-cutting tracks (never "done", always watched)

- **Proof-rate ratchet:** suite grows every sprint; any % drop on existing
  cases fails CI even when tests pass.
- **Compile-time budgets:** release-build wall clock and hydration counts
  tracked per fixture; alarms at +10% week-over-week.
- **Docs-as-tests:** every doc example in `docs/tutorial` compiles via a
  snippet-extraction test (starts Sprint 2).
- **Multi-agent hygiene:** file-ownership map maintained in AGENTS.md; two
  workers must never own one file in the same window (real incident: W-CM
  edit race, resolved by integration — see Sprint 2 ADR trigger).

## 6. Risks

| Risk | Mitigation |
|------|------------|
| Silent deferral creep (unproven checks quietly accumulate) | Sprint 4 proof report + hard thresholds |
| Phase-order coupling in codegen | Sprint 2 single-source-of-truth ADR; invariant tests stay forever |
| Bare-name collisions as stdlib grows | Sprint 2 qualified identity |
| Verification-time blowups on quantifier-heavy contracts | budget alarms; case timeout + minimization harness |
| Parallel-agent merge thrash | ownership map + integration-first policy |

## 7. Non-goals (for now)

- A Rust-style borrow checker — Salt's thesis is *verify, don't restrict*.
- Garbage collection, dynamic runtimes, interpreter-first development.
- API stability guarantees before stdlib v1 (Sprint 6).

## 8. Changing this document

Roadmap edits are cheap; silent divergence is expensive. When reality overtakes
a sprint (it will), update the section, append a one-line note in the sprint's
retro slot, and keep the exit criteria honest rather than retro-fitting them to
what happened anyway.

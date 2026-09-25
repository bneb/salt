# Salt — Sprint Execution Queue

Instantiated goal prompts for SPRINT_ROADMAP.md sprints. One goal per sprint;
any agent executes cold. Dependencies run in ID order; unmet dependencies =
BLOCKED, not skip.

- **SALT-S1-CORRECTNESS-FOUNDATIONS** — COMPLETE (ghost-identity family closed at root; see ROADMAP.md S-ladder retros)
- **SALT-S2-ONE-TYPE-SYSTEM** — IN FLIGHT
  - `S2.0` Delete orphan registries & legacy dead paths — COMPLETE (`a52d3d2`, `22b4af0`)
  - `S2.1` `InstanceId` / `FnInstanceId` typed identity scaffold — COMPLETE (`b0d2dff`, `ede0353`)
  - `S2.2` `define_instance` registration chokepoint & validation — COMPLETE (`b496fbe`, `8c53b42`)
  - `S2.3a` Typed-identity side map in context & `GenericArg` fidelity — COMPLETE (`844722b`)
  - `S2.3b` Read from side map in method resolution & codegen lookup — NEXT
  - `S2.4` Canonical dispatch (ADR 016) & retire dual-sync snapshot paths — NEXT
  - `S2.5` Fully-qualified trait coherence keys `(package_path, trait_name)` — NEXT
- **SALT-S3-GENERICS-THAT-EARN-TRUST** — QUEUED (DEPENDS-ON: S2)
  - `S3.1` Generic trait impls (`impl<T> Trait for Struct<T>`) with inherited defaults
  - `S3.2` Const generics assumption propagation into Z3 solver contracts
  - `S3.3` Local type inference (WS-7) for receiver / call-site argument deduction
  - `S3.4` Monomorphization deduplication & timing ratchet baseline enforcement
- **SALT-S4-VERIFICATION-AS-PRODUCT** — QUEUED (DEPENDS-ON: S2)
- **SALT-S5-MEMORY-MODEL-CONSOLIDATION** — QUEUED (DEPENDS-ON: S3)
- **SALT-S6-TOOLCHAIN-AND-RELEASE-ENGINEERING** — QUEUED (DEPENDS-ON: S1)

## Template slots (per sprint)

| Slot | Source |
|------|--------|
| GOAL-ID | SALT-S{N}-{THEME-KEBAB} |
| OBJECTIVE | Sprint Goal line + exit criteria, verbatim |
| READ FIRST | Roadmap section + debt rows the sprint kills |
| PROBE FIRST | One falsifiable pre-check per major claim |
| DEFINITION OF DONE | Exit criteria + gates + retro paragraph |
| DEPENDS-ON | Header line; unmet = BLOCKED |

## Working agreements (every execution)

1. `cargo test` (salt-front/) — 0 failures; case count monotonic.
2. `cargo clippy --all-targets -- -D warnings` — zero warnings.
3. `bash salt-front/tests/z3_contracts/run_tests.sh` — 44/44 green.
4. `bash scripts/spelling_goldens_gate.sh` — 18/18 pinned expectations.
5. `bash scripts/proof_gate.sh` — proof ratio ≥ floor (45%) with 0 regressions.
6. `PASSES=3 bash scripts/mlir_determinism_gate.sh` — 33 fixtures byte-stable.
7. `bash scripts/fuzz_campaign.sh` — bounded AST generation; 0 panics / 0 ICEs.
8. Hard style constraints: ≤32 non-blank lines/fn, ≤500 lines/file, ≤3 nesting levels, zero mutants (TODO/FIXME).
9. VCS: nothing committed or pushed without explicit approval.

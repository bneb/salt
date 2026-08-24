# Salt — Sprint Execution Queue

Instantiated goal prompts for SPRINT_ROADMAP.md sprints. One goal per sprint;
any agent executes cold. Dependencies run in ID order; unmet dependencies =
BLOCKED, not skip.

- **SALT-S1-CORRECTNESS-FOUNDATIONS** — DISPATCHED (in flight)
- SALT-S2-ONE-TYPE-SYSTEM — DEPENDS-ON: S1
- SALT-S3-GENERICS-THAT-EARN-TRUST — DEPENDS-ON: S2
- SALT-S4-VERIFICATION-AS-PRODUCT — DEPENDS-ON: S2
- SALT-S5-MEMORY-MODEL-CONSOLIDATION — DEPENDS-ON: S3
- SALT-S6-TOOLCHAIN-AND-RELEASE-ENGINEERING — DEPENDS-ON: S1

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

cargo test 0 failures · z3 suite green and monotonic · clippy `-D warnings`
clean (S1 upgrades to `--all-targets`) · style caps (≤32/≤500/≤3) · proof
ratchet (no increased silent deferral) · nothing committed without approval.

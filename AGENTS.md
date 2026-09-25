# Salt — Agent Instructions

## Build & Test
- `cargo build --release` in salt-front/ for the compiler
- `cargo test` in salt-front/ for unit tests
- `cargo clippy -- -D warnings` before commits
- `bash salt-front/tests/z3_contracts/run_tests.sh` for contract tests
- `bash scripts/spelling_goldens_gate.sh` for emission-identity goldens
- `bash scripts/proof_gate.sh` and `PASSES=3 bash scripts/mlir_determinism_gate.sh` for proof/determinism ratchets

## Hard Constraints
- Never add `Co-Authored-By`, `Signed-off-by`, or any git trailer attributing work to Claude/Anthropic
- Max 32 non-blank lines per function
- Max 500 lines per file
- Max 3 levels of indentation nesting
- No mutants (TODO/FIXME/HACK/XXX/temp_/workaround) in non-test source
- Every module must have a corresponding test file

## Architecture Invariants
- Z3 contracts on public functions are non-negotiable — every unsafe operation needs a `requires` clause
- Never commit, push, or deploy without explicit permission

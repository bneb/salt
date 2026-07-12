# Salt Backend Tests

**The Mission:** Verify the correctness of the MLIR lowering and Z3 verification passes.

## Components

| File | Role |
|------|------|
| [`run_execution.py`](./run_execution.py) | **Test Runner.** Executes the Salt compiler on test files and checks output. |
| [`verification_pass.salt`](./verification_pass.salt) | **Z3 Positive.** Code that *should* pass verification. |
| [`verification_fail.salt`](./verification_fail.salt) | **Z3 Negative.** Code that *should* trigger a compile-time error. |

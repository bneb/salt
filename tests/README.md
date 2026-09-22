# Test Suite

**The Mission:** Ensure the compiler and standard library strictly adhere to the KeuOS Invariants.

## Structure

| Directory | Role |
|-----------|------|
| `*.salt` (root) | **Integration tests.** End-to-end compiler tests covering hashmap, arena, combinators, file I/O, println, f-strings, iterators, proof witnesses, etc. |
| [`unit/`](./unit) | **Unit tests.** Focused tests for individual language features (18 test files). |
| [`cases/`](./cases) | **Test cases.** Grouped test scenarios (e.g. `hashmap_test.salt`, `string_test.salt`). |
| [`lib/`](./lib) | **Library/module tests.** Cross-module linking and symbol mangling (e.g. `test_lib_mangling.salt`, `test_cross_module_struct_lib.salt`). |
| [`bridges/`](./bridges) | **C interop bridges.** `.c` files used by FFI-facing tests (e.g. `gc_bridge.c`, `chronos_bridge.c`). |
| [`fixtures/`](./fixtures) | **Test input data.** Non-Salt fixtures other tests read (images, CSS, JSON). |
| [`bench/`](./bench) | **Latency/perf tests** (e.g. `bench_arbiter_latency.salt`, `jitter_isolation.salt`). |
| [`chaos/`](./chaos) | **Stress tests** (e.g. `reclamation_storm.salt`). |
| [`v5_isolation/`](./v5_isolation) | **V5 isolation tests.** Module isolation and linking tests. |
| [`v6_vector/`](./v6_vector) | **V6 vector tests.** SIMD and vectorization tests. |

> Descriptions for `cases/`, `lib/`, `bridges/`, `fixtures/`, `bench/`,
> `chaos/` are inferred from a sample of filenames in each directory,
> not read file-by-file — treat as a rough map, not a verified spec.
> This table previously listed `regression/`, `keuos/`, and
> `snapshots/`, none of which exist in this checkout; removed rather
> than guessed at.

## Run Tests
```bash
cd salt-front && cargo test
```

Individual Salt test files can be compiled and verified directly
(`tests/sanity.salt` referenced here previously doesn't exist in this
checkout; `--lib -o /dev/null` just runs verification and discards the
MLIR, which is what this is actually demonstrating):
```bash
./salt-front/target/release/saltc tests/test_json_objects.salt --lib -o /dev/null
```

# Compiler findings from auditing Facet

Notes from making a real Salt program ([Facet](https://github.com/bneb/facet),
a 2D compositor) **link and run** on macOS/Metal. Every issue below was
reproduced against `9879c5e` on arm64-darwin (Apple clang 21), and each comes
with a minimal, self-contained reproduction.

The reason these surfaced: almost everything that uses Salt type-checks but is
never executed. Facet's `make test` ran `saltc <file> --lib -o /dev/null` on
four files and exited 0 — for a whole session I believed the code was sound.
Getting a Salt binary to actually *run* is what turned latent miscompilation
into immediate, visible test failures.

Reproduce everything below with:

```sh
cargo build --release                      # needs Z3_SYS_Z3_HEADER set
SALTC=./salt-front/target/release/saltc
# each repro is a self-contained .salt file; see "How to run" at the bottom
```

---

## P0 — Silent miscompilation of float code

These produce **wrong answers with no diagnostic**. They are the reason a
green type-check told us nothing.

### 1. An `f32` struct field initialised from a literal reads back `0.0`

```salt
extern fn exit(code: i32);
struct S { d: f32, n: i64 }

pub fn main() -> i32 {
    let s = S { d: 1.0, n: 5 };
    if s.n != 5 { exit(3); }
    if s.d == 1.0 { exit(0); }   // NOT taken
    exit(4);
}
```

Observed: **exit 4**. `s.d` is `0.0`, not `1.0`.

The `i64` field in the same literal is fine, so this is specific to float
fields. It corrupts any tagged-union / tagged-struct style code that stores a
float in a struct, which is a very common shape (colour, transform, geometry).

### 2. A comparison of two `f32` **parameters** always returns true

```salt
extern fn exit(code: i32);
fn is_down(y0: f32, y1: f32) -> bool { return y0 < y1; }

pub fn main() -> i32 {
    let ten: f32 = 10.0;
    let thirty: f32 = 30.0;
    if !is_down(ten, thirty) { exit(1); }   // not taken: 10 < 30, correct
    if is_down(thirty, ten) { exit(2); }    // TAKEN: 30 < 10 is false
    exit(0);
}
```

Observed: **exit 2**. `is_down` returns true for both orderings.

A comparison of two `f32` **literals** (`30.0 < 10.0`) is correct — I verified
this separately. The broken path is the one through parameters. This is far
worse than bug 1: every comparison in a float-taking function is a coin flip,
with no error.

**How this reached production in Facet.** `add_line_edge` normalises a line
edge to top-to-bottom:

```salt
if fabsf(y1 - y0) < 0.001 { return; }          // skip horizontals
if (y0 < y1) { push(Edge { ..., dir: 1.0 }); }
else         { push(Edge { x0: x1, y0: y1, ... dir: -1.0 }); }
```

Printing the resulting edge table at the C FFI boundary shows the else branch
taken for *every* edge and the horizontal skip never firing:

```
e0: x0=30 y0=10 x1=10 y1=10 dir=-1   <- horizontal, must be skipped
e1: x0=30 y0=30 x1=30 y1=10 dir=-1   <- should be unswapped with dir=+1
e2: x0=10 y0=30 x1=30 y1=30 dir=-1   <- horizontal, must be skipped
e3: x0=10 y0=10 x1=10 y1=30 dir=-1
```

Metal then renders that table faithfully, so a 20×20 rectangle comes out as
40×20 — and across repeated runs the fill alternates between that and drawing
nothing at all. I confirmed the *original, unmodified* sources produce
identical output, so this is long-standing, not a recent regression.

**Suggested areas to look at:** `src/codegen/expr/` float promotion and the
`prepare_receiver_arg` / `bind_receiver_params` paths that rewrite parameter
types. The symptom (a comparison that ignores operand order) suggests a
parameter is being coerced through a path that collapses it to a constant, or
a flag operand is being dropped so the comparison degenerates.

### 3. A method whose name matches an extern it calls recurses into itself

```salt
extern fn free(p: Ptr<u8>);
struct Path { cmds: Ptr<PathCmd>, count: i64, capacity: i64 }
impl Path {
    pub fn free(&mut self) {
        free(self.cmds as Ptr<u8>);   // intends libc free
        self.count = 0;
    }
}
```

The emitted **MLIR is correct** — `func.call @free(...)`. But the object file
is not:

```
_raster__Path__free:
	ldr	x8, [x0]
	mov	x0, x8
	bl	_raster__Path__free    ; <- itself, not libc free()
```

Infinite recursion. The bug is introduced somewhere between MLIR and the
object, and it is silent: the MLIR-level review is clean, so a reviewer
checking the IR would not catch it.

**Workaround (validated):** rename the method — `Path::dispose`, `Canvas::dispose`,
`EdgeTable::dispose` — keeping the public free-function wrappers unchanged. The
call then correctly targets `symbol stub for: _free` and the program exits 0.
This is a real constraint on user code today: **a Salt method cannot be named
the same as a libc function it calls.**

---

## P1 — Toolchain that cannot produce a runnable binary

### 4. `saltc --binary` fails to link on macOS

```
Undefined symbols for architecture arm64:
  "__NSGetArgc", referenced from: _salt_get_argc in runtime.o
  "___adddf3",   referenced from: _raster__flatten_cubic
```

The runtime needs `-lobjc` (Foundation) and the generated code can reference
soft-float helpers. The tool's own link step does not supply either, so
`--binary` is unusable out of the box and the object is left behind in the
temp build dir.

**Workaround (validated):** link manually.

```sh
clang -fobjc-arc -o prog \
  $SALT_BUILD/prog.o $SALT_BUILD/runtime.o \
  -lobjc -liconv -framework Cocoa -framework Foundation \
  -framework Metal -framework QuartzCore
```

This is what let me run Facet's test suite for the first time. Making
`--binary` do this itself would meaningfully raise the number of Salt programs
that are ever executed.

### 5. An untyped float literal in a `let` is emitted as `f64`

```
math.mlir:74:37: error: use of value '%f1' expects different type than prior uses: 'f32' vs 'f64'
    %math_fabs_4 = "llvm.intr.fabs"(%f1) : (f32) -> f32
    %f1 = arith.constant -1.00000000000000000e0 : f64
```

`let neg = -1.0;` defaults to `f64`; passing it to `fabsf(x: f32)` is a hard
error. Writing `let neg: f32 = -1.0;` avoids it. Given bugs 1 and 2, **f32
typing deserves a dedicated pass** rather than a default that flips per site.

---

## P2 — Correctness hazards in the build/CI surface

### 6. `--lib` silently drops every function body

`saltc prog.salt --lib -o out.mlir` emits the `extern fn` declarations and the
stdlib preamble, and **no user function bodies**. Z3 reports `0/0 checks`, and
a program calling two functions that do not exist compiles successfully:

```
Z3: 0/0 checks proven (0%), 0 deferred to runtime
✅ MLIR compiled successfully.        (exit 0)
```

This is arguably by design, but it is a trap: `saltc --lib` looks like a
type-check and is not one. For `package main` files it is a silent no-op on
the entire program. A `--lib` build that emits zero non-extern functions could
at minimum warn, and I would suggest documenting the distinction in `--help`
(the current help text just says "Library mode (no main entry point
required)").

### 7. The released 1.2.0 binary has a broken embedded stdlib

`saltc 1.2.0` from `cargo install` fails on essentially every file:

```
[E003] Compilation failed:
Undefined function or symbol: 'Slice__new'
Method call 'write' requires a receiver value
```

Impl methods from the **bundled** `std.core.*` modules never register, so
`Slice::<u8>::new`, `Ptr.offset`, and `Ptr.read`/`write` all fail to resolve.
Building from git HEAD works correctly. The divergence between what
`build.rs` bundles and what HEAD's `std/` contains is worth a check in CI — a
smoke test that compiles one file using `Slice` would have caught it.

### 8. `std/io/print.salt` imports a module path that does not exist

```
[W008] could not find imported module 'std.fmt'
```

`std/io/print.salt:6` has `import std.fmt.{Display, format};` but the module
is `std.core.fmt` (`std/core/fmt.salt`, `package std.core.fmt`). The compiler
handles it gracefully via the `std.core.X.Y -> std/core/X.salt` fallback in
`cli.rs`, so it is a warning rather than an error — but it fires on **every
single compile** and should be a one-line fix.

### 9. Local `impl` blocks on locally-defined structs fail to compile

```salt
struct P { p: Ptr<u8>, n: i64 }
impl P { pub fn mk() -> P { ... } }
```

```
[E003] Compilation failed:
Failed to derive TypeKey for impl target P
```

This blocked me from writing a *minimal* reproduction of bug 3 (I had to go
through an imported module instead). It also means a single-file Salt program
that defines a struct with methods cannot be compiled at all, which is a
significant limitation for test files and examples.

---

## Things I checked that are **not** bugs

Recording these so they don't get chased:

- **`f32` comparisons between two literals are correct.** `30.0 < 10.0` is
  false, as expected. Only the parameter path (bug 2) is broken.
- **`@shader` support is real.** `thread_id()`, `read_at()` and `write_at()`
  are special-cased in `src/codegen/shader.rs` and work; they are not missing
  from the stdlib. I initially reported these as nonexistent by checking only
  `std/core/ptr.salt` and was wrong.
- **The Z3 coverage numbers are honest.** `raster.salt` reports 8/96 proven,
  95 deferred. The deferrals are real, not a reporting artefact.

## One process note

Every one of the P0 bugs passed a CI gate. Facet's gate was
`saltc <file> --lib -o /dev/null`, and it stayed green through all of them.

A gate that only parses is worse than no gate, because it converts "unverified"
into "verified" in everyone's mental model. Two cheap changes that would have
caught these immediately:

1. **Run the tests.** The programs compiled fine; they had just never been
   executed. A type-check is not a substitute.
2. **Make the gate provably able to fail.** Add a deliberately-broken control
   file to CI that must be rejected. A gate whose own failure mode has never
   been demonstrated is an assumption, not a check.

## How to run

Each repro above is standalone. Given `prog.salt`:

```sh
$SALTC prog.salt --binary -o /tmp/prog 2>&1 | tail -2   # link fails (bug 4)
clang -o /tmp/prog $SALT_BUILD/prog.o $SALT_BUILD/runtime.o \
  -lobjc -liconv -framework Cocoa -framework Foundation
/tmp/prog; echo "exit=$?"
```

`$SALT_BUILD` is the temp dir saltc names in its own output; the `.o` is still
written there when its internal link fails.

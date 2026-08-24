# Standard Library Health Audit

**Date**: 2026-02 working session (round 4)
**Method**: every .salt file under salt-front/std/ compiled standalone:

```bash
cd salt-front
for f in $(find std -name '*.salt' | sort); do
  ./target/release/saltc "$f" --lib --disable-alias-scopes -o /dev/null \
    && echo "PASS $f" || echo "FAIL $f"
done
```

**Result at audit time**: 64 PASS / 37 FAIL (101 files).
**Result after first fix**: 65 PASS / 36 FAIL (std/io/print.salt, below).
**After enum-resolution work (generic ctors + value path + prefix-tolerant
expected-type unification + removal of the silent specialization fallback)**:
**73 PASS / 28 FAIL**. Newly compiling (7): std/core/iter, std/env, std/json,
std/fs, std/channel, std/io/file, std/sync/ring_buffer. A later
structural-equality refinement also re-healed std/http/client.
Claimed-but-NOT-healed (3): http/client (method-lowering gap: `Method data
not found on StringView`), io/mod (transitive: writer import skip + Z3
`(>= len 0)` precondition), io/writer ([E002] parse `expected requires`).
Each straggler's blocker is a distinct pre-existing class, not enum resolution.

FINAL (post method-resolution, intrinsic-table, grammar, and kernel-tree
healing): **101 PASS / 0 FAIL of 101**. Every std module compiles standalone
under --lib with Z3 verification on. Healed after that checkpoint:
http/client (StringView::data accessor + structural-eq conformance),
io/mod + io/writer (grammar: var locals, attr dispatch, trait defaults;
source FFI externs and match discriminants), keuos/sovereign tree (kernel
externs, bare intrinsics, XOR-NOT form, statement-position unsafe),
comptime (typed var locals + attribute dispatch + source modernization),
syscalls (missing semicolon), random (static fn parser support), alloc
(trait default-method parsing).
**Historical checkpoint (mid-session)**: 65 PASS / 36 FAIL after the
print.salt fix, before the enum-resolution work landed. The probe matrix for
that state lives in the session review record; current counts are at the top
of this document.

## Fixed during this audit

### std/io/print.salt - hello-world path was broken

Three distinct defects, each masked by the previous one failing first:

1. print_str indexed a raw pointer in a null-terminator scan without an
   unsafe block. Z3 correctly refused to prove unbounded pointer safety.
   Fix: wrap the scan in unsafe { }, matching the existing idiom in
   std/string.salt push_cstr.
2. All four println_* variants passed a string literal newline directly to
   sys_write(fd: u64, buf: u64, len: u64). Literals lower to StringView
   structs and cannot promote to u64; no other std module passes literals
   to externs. Fix: local 'let nl: [u8; 2] = [10, 0];' and pass &nl[0] with
   length 1, preserving exact write semantics.

Verified by: module compiles standalone; a caller program exercising
print_int/println_int compiles through full codegen; all 7 examples/*.salt
compile; Z3 contract suite 44/44 green after the change.

## Failure classes encountered (all resolved; historical reference)

Classification from error-message sampling - fix in this order:

| Class | Modules (confirmed) | Symptom | Likely fix locus |
|-------|---------------------|---------|------------------|
| GENERIC enum constructors unresolved | RESOLVED this session [was: core/iter, core/args, env, fs, channel, json] | qualified ctors on generic enums now compile; see probe matrix in the review record | FIXED via utils.rs/enum_ctor.rs rework + literals.rs imported-template fallback + prefix-tolerant expected unification |
| Parse-level rejection | random/mod (`static fn` in impl), comptime.salt (construct unidentified), core/alloc.salt (trait methods WITH bodies unsupported - verified; bodyless ones need `;`, fixed) | `[E002] failed to parse '<path>': expected ...` | grammar gaps: `static fn`; defaulted trait methods |
| Kernel-target-only sources | syscalls.salt, io/arch/*, io/reactor_*, core/keuos/** | parse or resolution errors on kernel syntax | expected userspace failures; gate behind keuos target |
| Method-call / method-store lowering | fmt/display.salt, core/conv.salt | `Method call 'get' requires a receiver value`; `Method store not found on type Ptr` | codegen method-call path |
| Numeric promotion gaps | hash/mod.salt | `Numeric promotion not supported from F64 to U64` in f64__hash | promotion rules or explicit cast in source |
| Intrinsic signature mismatch | encoding/encoding.salt | `Intrinsic 'ptr_write_at' emission failed: expects 2 arguments` | intrinsic name-to-signature table |

### Root cause isolated: GENERIC enums only (construction matrix)

| Case | Result |
|------|--------|
| local non-generic `enum Opt { Some(i32), None }`, call `Opt::Some(5)` | compiles (exit 0) |
| local GENERIC `enum Opt<T> { Some(T), None }`, call `Opt::Some(5)` | FAILS `Undefined function or symbol: 'main__Opt__Some'` |
| imported generic `Option::Some(5)` after `use std.core.option.*` | FAILS same class |
| unqualified `Some(5)` | FAILS (unqualified variant calls unsupported) |
| struct literal `P { x: 5 }` | compiles (exit 0) |

Conclusion: qualified constructor calls work for non-generic enums and fail for
generic ones - independent of imports. Since std Option/Result are generic,
the language's entire error-handling construction surface is broken. Suspect:
template lookup in resolve_path_to_enum never finds ctx.enum_templates entries
keyed under mangled generic names (population site codegen/mod.rs:1762), or an
early `return None` on the expected_ty branch at utils.rs ~331-339.
Highest-priority compiler fix; landed round 13 (utils.rs rework + new
expr/enum_ctor.rs): direct qualified construction now works for local and
imported generic enums, including the tutorial's Result::Ok/Err story.
Residual subclasses tracked as tickets: (1) constructor arguments that are
non-trivial expressions (`Some(1 + 2 * 3)`) are not traced yet - pinned by
negative test test_expression_arg_not_yet_inferred_is_rejected; (2)
value-position unit variants (`let x = Option::None;`), dominant cause of
the remaining iter/env/args failures (literals.rs single-import mangling at
~L484 vs group branch ~L477); (3) generic-enum ctors inside impl methods
whose argument is an inline call expression (fs/channel Err sites).
Negative-test hygiene note: unused `let` bindings are dead-code-eliminated
before resolution, so negative tests must FORCE resolution (match on the
value) or they silently pass.

BREAKING CHANGE (sanctioned pre-1.0): the silent specialization fallback for
payload-blind constructors was REMOVED. `Result::Err(Status)` with no other
binding for T is now a clean compile error instead of silently adopting an
arbitrary prior specialization (demonstrated mis-typing: an earlier
`Ok(true)` made a later bare `Err(st)` become `Result<bool>`). Annotate the
type or use a payload-typed variant. Compensating fix: return-position and
annotated expected types now unify through prefix tolerance, which is what
healed the six modules above.

Round-13b addition - VALUE-position unit variants partially fixed (ticket 2):
`literals.rs` now consults imported-template registry entries and the expected
type for `Enum::Variant` value paths. `iter.salt` and `env.salt` compile
(67/34 overall). Still rejected by design: UNANNOTATED value-position None
with no expected type (`let x = Option::None;`) - T is genuinely ambiguous
without flow inference; annotate or return it. Ticket 1 (expression args)
and ticket 3 (impl-method call-arg clobbering) remain open.

## Diagnostic notes

- Parse failures now emit `[E002] failed to parse '<path>': <detail>` (wired
  through src/errors.rs during the round-7 diagnostics work). Earlier bare
  `Error: expected ...` output predates that change.
- Measurement gotcha when reproducing: piping saltc stderr through `head` makes
  `$?` report head's exit status, not saltc's. Redirect instead of piping when
  scripting gates.
- Prelude imports are working-directory sensitive: compiling from `salt-front/`
  resolves cleanly, while from the repo root a chained std import (e.g. std.fmt
  via io.print) emits `[W008]` yet compilation still succeeds. Pre-existing
  resolution quirk, unrelated to the round-7 diagnostics renaming.
- SPEC divergence (section 7, line ~214): the spec documents UNQUALIFIED enum
  construction (`VariantName(vals)`); the implementation only resolves the
  QUALIFIED path (`Enum::Variant(...)`) today, and even that only for
  non-generic enums pre-fix. After the generic-constructor fix lands, either
  implement the spec's unqualified form or amend the spec - one of the two must
  move for the language to match its own reference.
- Tutorial impact: docs/tutorial/06-error-handling.md's flagship example does
  not compile today - `Result::Ok(...)` / `Result::Err(...)` hit exactly this
  generic-enum bug (`Undefined function or symbol:
  'std__core__result__Result__Ok'`, reproduced from the doc's own snippet).
  Post-fix acceptance test: that tutorial file compiles as written.
  STATUS: RESOLVED - the tutorial snippet compiles (verified directly), and
  the generic-enum fix is landed with red-team review.

Round-13b additions:
- Ticket 1 (expression args) RESOLVED: tracer.rs gained Binary/Unary/Paren
  arms; binops statically yield the wider operand type, comparisons Bool.
  Pinned by test_expression_arg_infers_wider_int_type in
  tests/generic_enum_ctor_test.rs.
- Ticket 2 partially resolved: VALUE-position variants via imported templates
  now specialize from the expected type (literals.rs try_imported_enum_template).
  UNANNOTATED unused `let x = Option::None;` remains rejected by design (T is
  ambiguous without flow inference) - clean E003, documented here so nobody
  mistakes it for the old bug.
- Tickets 3 (impl-method inline-call clobbering) and the grammar gaps remain open.
- NEW TICKET (pre-existing, high priority for the verification story):
  payload-slot conformance is unchecked post-specialization. Probe:
  `fn -> Result<i64> { return Result::Ok("boom"); }` COMPILES - the string
  literal is stored as i64 into the payload buffer and match returns those
  bytes as an integer. Turbofish args are silently overridden by expected
  types too (`Result::<i64>::Ok(1)` inside `-> Result<bool>` becomes
  Result<bool>). Fix: after specialize_template_variant, verify each ctor arg
  against its substituted payload slot; improve mismatch diagnostics.
  STATUS: IMPLEMENTED AND VERIFIED (enum_ctor.rs verify_ctor_arg_types). Semantics:
  identical types pass; numeric pairs pass ONLY where emission really coerces
  them (int->int widening/narrowing casts, int->float via sitofp); a float
  value into an integer slot is rejected up front with a clean [E003]
  mismatch error instead of failing late in promote_numeric ('Numeric
  promotion not supported from F64 to I64'); everything else is a clean [E003]
  mismatch error. Comparison uses Type::structural_eq so equivalent
  representations (Struct vs zero-arg Concrete) do not false-positive.
  Reference-typed args are auto-deref'd one level to match promote_numeric.
  The registry fast path (already-specialized enums) runs the SAME per-arg
  conformance since B2; it previously bypassed every check. Tracer-gap args
  (untraceable) are skipped and left to emission diagnostics - the
  while-invariant contract test covers this skip path (Status::from_code ctor
  arg inside std/core/result usage). Fn-item args (Ok(g), bare fn) were ALSO
  skipped and silently stored the fn address as the payload; CLOSED: the
  tracer now types bare fn names as Type::Fn (tracer_lowering trace_path
  probes discovery.globals under the mangled fn key) and conformance rejects
  fn items in any non-fn-pointer slot with a clean [E003] mismatch naming the
  wrap-the-call fix. Genuine fn items into `fn(...)->...` slots still coerce
  at emission. Locked by tests/fn_item_payload_conformance_test.rs and the
  flipped pin test_fn_item_ctor_arg_rejected in tests/generic_enum_ctor_test.rs.

- NEW TICKET (proven blocker for fully generic traits): GENERIC STRUCT FIELD
  ACCESS inside impl methods fails on erased self-types. Repro A (direct
  impl): struct Slot<T> { v: T } + impl Slot<i64> { fn doubled(&self) -> i64
  { return self.v * 2; } } => [E003] Cannot access field Member::Named(v) on
  type Reference(Struct("main__Slot")). Field lookup does not substitute
  generics for template structs reached via &Self. Repro B (trait impl):
  same shape under `impl Prod for Slot<i64>` fails EARLIER with "Template
  Slot not found in registry" - the trait-impl path cannot even locate the
  generic template. Fix locus suspects: resolve_field_type (tracer.rs -
  coordinate with active owner) and/or seeker/typeck generic substitution;
  template registration for trait-impl receivers separately.

- NEW TICKET (round-13b diagnosis, blocks the same 8 modules): for
  `Result::Err(...)` calls inside trait-impl modules, resolve_path_to_enum is
  NEVER INVOKED - stage tracing proved the call reaches identify_target's
  fallthrough directly ([DBG-FT] tagged). Something upstream of the enum
  branch in resolve_standard_call short-circuits for these receivers. Fix
  locus: whatever dispatches method/call resolution before the enum branch;
  NARROWED (round-2 tracing): lookup_template_triple now serves imported
  triples correctly; the failure has MOVED downstream - emission of the
  resolved ctor still targets the unspecialized symbol. Next fix locus:
  the EnumConstructor emission path must use the specialized symbol.

> Note: kernel-class modules may be *expected* userspace failures. Their true
> health metric is compilation under the KeuOS target pipeline, not --lib.
> They are listed for completeness, not necessarily as defects.

## Reproducing

The one-liner above is the whole test. Plain --lib is the portable gate;
--danger-no-verify cannot weaken it in release builds (by design).

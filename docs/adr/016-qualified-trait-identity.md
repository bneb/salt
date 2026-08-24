# ADR 016: Qualified Trait Identity for Method Dispatch

**Status:** Accepted
**Date:** 2026-08
**Deciders:** Salt compiler design

## Context

Two modules may each declare a trait with the same bare name (`Greeter` in
package `a` and package `b`), and impl scanning flattens every impl into one
per-compilation method registry. Dispatch must therefore answer: *which*
module's `greet` runs for a given call site? Two canonicalization points
compete:

- **AST-canonical**: key methods by names as written at each call site
  (`receiver name + method name`), re-resolving imports per lookup.
- **HIR-canonical**: key methods by identities fixed once during discovery —
  fully-qualified receiver types (`TypeKey{path, name}`, mangled
  `pkg__mod__Type`) plus the method name and a parameter-signature hash.

The registry is written by impl discovery with the defining module's path
attached, but read by lowering, hydration, and monomorphization tasks that do
not all see the same import sets.

## Decision

**HIR-canonical dispatch.** Registry keys carry qualified receiver identity,
and every lookup prefers the fully-qualified match before any bare-name
fallback.

Concretely:

1. `trait_registry.rs::MethodKey` =
   `(receiver_type: TypeKey, method_name, param_signature_hash)`;
   `TypeKey.path` holds the defining module path, stamped when the scanner
   registers each impl.
2. The legacy fallback chain ends in `find_method_by_name` →
   `pick_method_by_identity`, which ranks candidates: an exact mangled
   receiver match (`pkg__mod__Type`) always outranks a bare-name match;
   equal ranks break deterministically by receiver mangle.
3. Path expressions canonicalize to their true home before dispatch:
   `Enum::Variant` calls resolve against the enum's home module rather than
   the caller's package guess via `resolve_path_to_enum`
   (`src/codegen/expr/resolver.rs`) → `lookup_template_triple`
   (`enum_ctor.rs`, accepting `fqn`/`pkg_mangled`/`doubled` spellings of
   imported templates), with `unify_expected_type` tolerating the
   doubled-leaf form (`pkg__Result__Result`) by structural prefix.

No further compiler change was required: end-to-end behavior is proven by
`tests/trait_qualified_identity_test.rs` — same-named traits across packages
on distinct types (each sentinel body emitted under its own qualified
symbol), generic variants, single-module regression, plus imported-function
and imported-generic-enum construction inside trait-method bodies.

## Alternatives Considered

- **AST-canonical (bare-name) keys**: rejected. Same-named traits collide in
  the registry; last registration wins and calls silently dispatch to the
  wrong body. This was the original failure mode.
- **Trait-name-first tables with per-trait vtables** (Rust-style trait
  objects): rejected as a redesign far exceeding current needs; Salt has no
  dynamic dispatch yet, and receiver-first resolution already yields correct
  static dispatch.
- **Require fully-qualified paths at every lookup**: rejected because
  hydration tasks legitimately lose the defining module's wildcard imports; a
  hard requirement would break monomorphized bodies. The ranked bare-name
  fallback keeps those callers working deterministically.

## Consequences

- **Positive**: Same-named traits and types coexist across modules; each call
  resolves to its own module's impl with its own body constants.
- **Positive**: Resolution is deterministic: qualified rank first, stable
  tie-break by mangled name.
- **Negative**: The bare-name fallback remains order-dependent if two
  same-named receivers ever register with no path context at all (ranked
  deterministically, but not semantically disambiguated).
- **Negative**: Coherence bookkeeping (`trait_impls`, `trait_origins`) still
  keys traits by bare name, so two distinct same-named traits implemented for
  the *same* type would report a duplicate implementation; acceptable until
  trait identity gains full path qualification there too.
- **Neutral**: `param_signature_hash` hashes `Debug` formatting via
  `DefaultHasher`; unstable across process runs but only compared within one
  compilation.

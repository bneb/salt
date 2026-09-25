# Const-Generic Parameter Limitations

## Struct Literals Cannot Bind Const Params

When a generic struct takes `const` parameters, struct *literals* cannot
carry turbofish arguments. Use a constructor function instead.

**Not supported** (syn::ExprStruct carries no turbofish):
```salt
let t = Tag::<true> { v: 42 };        // ← parsed as comparison, not turbofish
```

**Supported** (constructor fn binds params via call-site inference):
```salt
fn make(v: bool) -> Tag<true> { return Tag { v: v }; }
let t = make(true);                    // ← works: true binds K via unification
```

## Allocator Placeholder Forks Instance Keys

When a generic struct's trailing parameter (e.g., allocator) remains
unbound after all inference phases, competing instance spellings may be
registered (`Vec_i64_A` vs `Vec_i64_main__A`). The compiler emits a
near-miss hint on Undefined-struct errors to surface these. Resolution
(default-binding vs explicit-turbofish-only) is deferred pending a
language-level decision.

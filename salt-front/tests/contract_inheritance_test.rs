// Regression tests: CONTRACT INHERITANCE for trait defaults (item 7,
// stage 1). A default method's requires/ensures clauses ride along when
// inherited; an override DROPPING clauses the default carried is a hard
// [E009] -- silent weakening was previously accepted.
use saltc::compile;

const WEAKEN_MSG: &str = "weakens requires clause #1";

/// Override dropping the default's requires => hard error.
#[test]
fn override_dropping_default_requires_rejected() {
    let src = r#"
        package main

        trait Guarded {
            fn core2(&self, k: i64) -> i64;
            fn wrapped(&self, k: i64) -> i64
                requires(k > 0)
            {
                return self.core2(k);
            }
        }

        struct Thing { v: i64 }

        impl Guarded for Thing {
            fn core2(&self, k: i64) -> i64 { return k + self.v; }
            fn wrapped(&self, k: i64) -> i64 { return k; }
        }

        pub fn main() -> i32 {
            let t = Thing { v: 1 };
            return t.wrapped(-5) as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    let err = format!("{}", result.expect_err("weakening override must be rejected"));
    assert!(err.contains(WEAKEN_MSG), "expected weakening diagnostic, got: {}", err);
}

/// Inherited default keeps its contract end to end; an override that
/// PRESERVES the clauses stays legal.
#[test]
fn inherited_default_and_preserving_override_accepted() {
    let src = r#"
        package main

        trait Guarded {
            fn core(&self, k: i64) -> i64;
            fn wrapped(&self, k: i64) -> i64
                requires(k > 0)
            {
                return self.core(k);
            }
        }

        struct Thing { v: i64 }

        impl Guarded for Thing {
            fn core(&self, k: i64) -> i64 { return k + self.v; }
            fn wrapped(&self, k: i64) -> i64
                requires(k > 0)
            {
                return self.core(k) + 1;
            }
        }

        pub fn main() -> i32 {
            let t = Thing { v: 1 };
            return t.wrapped(5) as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "preserving override must compile: {:?}", result.err());
}

/// Stage 2 (Z3 refinement): same CLAUSE COUNT but weaker semantics --
/// `k != 0` admits negatives that `k > 0` forbids -- must now be caught.
#[test]
fn semantically_weaker_override_rejected() {
    let src = r#"
        package main

        trait Guarded {
            fn core2(&self, k: i64) -> i64;
            fn wrapped(&self, k: i64) -> i64
                requires(k > 0)
            {
                return self.core2(k);
            }
        }

        struct Thing { v: i64 }

        impl Guarded for Thing {
            fn core2(&self, k: i64) -> i64 { return k + self.v; }
            fn wrapped(&self, k: i64) -> i64
                requires(k != 0)
            {
                return self.core2(k);
            }
        }

        pub fn main() -> i32 {
            let t = Thing { v: 1 };
            return t.wrapped(-5) as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    let err = format!("{}", result.expect_err("semantically weaker override must be rejected"));
    assert!(err.contains(WEAKEN_MSG), "expected weakening diagnostic, got: {}", err);
}

/// Equivalent-but-differently-written clauses pass refinement -- the
/// point of stage 2 over syntactic comparison.
#[test]
fn equivalent_rewrite_accepted() {
    let src = r#"
        package main

        trait Guarded {
            fn core(&self, k: i64) -> i64;
            fn wrapped(&self, k: i64) -> i64
                requires(k > 0)
            {
                return self.core(k);
            }
        }

        struct Thing { v: i64 }

        impl Guarded for Thing {
            fn core(&self, k: i64) -> i64 { return k + self.v; }
            fn wrapped(&self, k: i64) -> i64
                requires(k >= 1)
            {
                return self.core(k) + 1;
            }
        }

        pub fn main() -> i32 {
            let t = Thing { v: 1 };
            return t.wrapped(5) as i32;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "equivalent rewrite must compile: {:?}", result.err());
}

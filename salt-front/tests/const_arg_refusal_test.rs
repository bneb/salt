//! WS-R3 T-b refusal (integration, full pipeline): integer literals in
//! const-generic position that do not fit i64 must fail compilation with
//! [E003] citing the digits, instead of silently dropping the argument
//! (which minted ghost `O_K` decls and ptr-typed unsuffixed calls). The
//! boundary twin keeps i64::MAX compiling at full width.
use saltc::compile;

const PROBE_HEAD: &str = "\
package main

struct O<const K: i64> { v: i64 }

impl<const K: i64> O<K> {
    pub fn mk() -> O<K> { return O { v: K }; }
}

pub fn main() -> i32 {
    let a = O::<";
const PROBE_TAIL: &str = ">::mk();
    return a.v as i32;
}
";

fn drive(literal: &str) -> Result<String, String> {
    compile(&format!("{}{}{}", PROBE_HEAD, literal, PROBE_TAIL), false, None, true)
        .map_err(|e| e.to_string())
}

#[allow(dead_code)] // RET7 amendment 1 uses this arm's message shape
fn compile_src(src: &str) -> Result<String, String> {
    compile(src, false, None, true).map_err(|e| e.to_string())
}

#[test]
fn overflow_literal_refuses_with_cited_digits() {
    let err = drive("99999999999999999999").expect_err("overflow must refuse");
    assert!(err.contains("does not fit in i64"), "missing class text: {}", err);
    assert!(err.contains("99999999999999999999"), "must cite digits: {}", err);
}

#[test]
fn first_unrepresentable_value_refuses_too() {
    let err = drive("9223372036854775808").expect_err("i64::MAX+1 must refuse");
    assert!(
        err.contains("does not fit in i64") && err.contains("9223372036854775808"),
        "unexpected refusal text: {}", err
    );
}

#[test]
fn i64_max_still_compiles_full_width() {
    let mlir = drive("9223372036854775807").expect("boundary value compiles");
    assert!(mlir.contains("9223372036854775807"), "full-width spelling lost");
}


#[test]
fn generic_receiver_chain_compiles_with_consistent_identities() {
    // NB-4/NB-5 completion: arity-tolerant unification + move hooks let a
    // Vec value flow through Vec::new()/push into a generic callee with
    // CONSISTENTLY param-spelled identities (values correct; WS-7 will
    // upgrade spellings to use-site names).
    const SRC: &str = r#"
        package main

        use std.collections.vec.Vec;

        fn len_of<U>(v: Vec<U>) -> i64 { return v.len(); }

        pub fn main() -> i32 {
            let mut v = Vec::new();
            v.push(1);
            v.push(2);
            let n = len_of(v);
            return n as i32;
        }
    "#;
    let mlir = compile(SRC, false, None, true)
        .expect("generic receiver chain must compile");
    // NB-6: allocator param removed from std Vec — identities are now
    // spelled WITHOUT the trailing A placeholder. Pin the structural
    // contract (both callee and method call present) without over-
    // constraining mangled spellings.
    assert!(mlir.contains("len_of"), "callee fn missing");
    assert!(mlir.contains("__push("), "specialized push missing");
    assert!(!mlir.contains("Unsupported explicit cast"), "cast error leaked");
    assert!(!mlir.contains("_T_A_"), "allocator fork identity leaked");
}

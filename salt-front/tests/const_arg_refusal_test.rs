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

#[test]
fn overflow_literal_refuses_with_cited_digits() {
    let err = drive("99999999999999999999").expect_err("overflow must refuse");
    assert!(err.contains("[E003]"), "missing [E003] prefix: {}", err);
    assert!(err.contains("does not fit in i64"), "missing class text: {}", err);
    assert!(err.contains("99999999999999999999"), "must cite digits: {}", err);
}

#[test]
fn first_unrepresentable_value_refuses_too() {
    let err = drive("9223372036854775808").expect_err("i64::MAX+1 must refuse");
    assert!(
        err.contains("[E003]") && err.contains("9223372036854775808"),
        "unexpected refusal text: {}", err
    );
}

#[test]
fn i64_max_still_compiles_full_width() {
    let mlir = drive("9223372036854775807").expect("boundary value compiles");
    assert!(mlir.contains("9223372036854775807"), "full-width spelling lost");
}

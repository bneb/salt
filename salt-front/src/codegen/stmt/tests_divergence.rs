//! Unit tests for divergence.rs. End-to-end coverage of the missing-return
//! rule is in tests/missing_return_test.rs.

use super::{breaks_to, check_main_final_expr, emit_while_exit};

fn expr(src: &str) -> syn::Expr {
    syn::parse_str(src).expect("expression parses")
}

#[test]
fn test_break_to_label_is_found() {
    assert!(breaks_to("    cf.br ^loop_exit_7\n", "loop_exit_7"));
}

/// Label ids are module-wide, so a nested loop's label can extend an
/// enclosing loop's.
#[test]
fn test_break_to_longer_label_is_not_a_break_to_its_prefix() {
    assert!(!breaks_to("    cf.br ^loop_exit_75\n", "loop_exit_7"));
}

#[test]
fn test_loop_condition_edge_is_not_a_break() {
    assert!(!breaks_to("    cf.cond_br %c, ^while_body_3, ^while_exit_4\n", "while_exit_4"));
}

#[test]
fn test_while_true_without_break_ends_block() {
    let mut out = String::from("    cf.cond_br %c, ^while_body_3, ^while_exit_4\n");
    assert!(emit_while_exit(&mut out, &expr("true"), "while_exit_4"));
    assert!(out.ends_with("  ^while_exit_4:\n    llvm.unreachable\n"), "{out}");
}

#[test]
fn test_while_true_with_break_continues() {
    let mut out = String::from("    cf.br ^while_exit_4\n");
    assert!(!emit_while_exit(&mut out, &expr("true"), "while_exit_4"));
    assert!(out.ends_with("  ^while_exit_4:\n"), "{out}");
}

#[test]
fn test_while_with_condition_continues() {
    let mut out = String::new();
    assert!(!emit_while_exit(&mut out, &expr("i < 3"), "while_exit_4"));
    assert_eq!(out, "  ^while_exit_4:\n");
}

fn main_fn(src: &str) -> crate::grammar::SaltFn {
    syn::parse_str(src).expect("fn parses")
}

#[test]
fn test_main_final_expression_is_rejected() {
    let err = check_main_final_expr(&main_fn("fn main() -> i32 { let x = 1; x + 1 }")).unwrap_err();
    assert!(err.contains("missing return in function 'main'"), "{err}");
}

#[test]
fn test_main_ending_in_statement_is_accepted() {
    assert!(check_main_final_expr(&main_fn("fn main() -> i32 { let x = 1; x + 1; }")).is_ok());
    assert!(check_main_final_expr(&main_fn("fn main() -> i32 { }")).is_ok());
}

// A function with a non-() return type must not reach the end of its body.
//
// Salt returns with `return` (docs/tutorial/02-functions.md); it never
// returns a block's final expression. saltc used to accept such a body and
// end it with `llvm.unreachable`, so a function like `is_some` below dropped
// its arm values and never returned: undefined behavior, which on arm64
// macOS traps. Only `main` returning i32 may reach its end (it returns 0),
// and not with a final expression it would drop.
//
// A path may still end without `return` when it cannot reach the end of the
// function: an infinite loop, or a call to C's `exit`/`abort`, as in std's
// Option::unwrap (see missing_return_divergence_test.rs).
//
// Each rejected function is called from main: an unreached function is
// never emitted, so it would compile for the wrong reason.

use saltc::compile;

fn missing_return_error(code: &str) -> String {
    let err = compile(code, false, None, true).expect_err("a body that can reach its end must not compile");
    format!("{err:#}")
}

fn assert_missing_return(code: &str, fn_name: &str) {
    let msg = missing_return_error(code);
    assert!(msg.contains(&format!("missing return in function '{fn_name}'")), "{msg}");
}

/// The MLIR from the definition whose header contains `header` up to the
/// next function.
fn fn_body<'a>(mlir: &'a str, header: &str) -> &'a str {
    let start = mlir.find(header).unwrap_or_else(|| panic!("{header} not emitted:\n{mlir}"));
    let rest = &mlir[start..];
    rest[1..].find("func.func").map_or(rest, |end| &rest[..end + 1])
}

/// The reported shape: arm values instead of `return`s.
#[test]
fn test_match_arm_values_are_not_returned() {
    assert_missing_return(r#"
        package main

        use std.core.option.*;

        fn is_some(opt: Option<i32>) -> i32 {
            match opt {
                Some(x) => 1,
                None => 0,
            }
        }

        pub fn main() -> i32 {
            let o: Option<i32> = Some(5);
            return is_some(o);
        }
    "#, "is_some");
}

#[test]
fn test_tail_expression_is_not_returned() {
    assert_missing_return(r#"
        package main

        fn ten() -> i32 {
            10
        }

        pub fn main() -> i32 {
            return ten();
        }
    "#, "ten");
}

#[test]
fn test_tail_if_expression_is_not_returned() {
    assert_missing_return(r#"
        package main

        fn pick(c: bool) -> i32 {
            if c { 10 } else { 0 }
        }

        pub fn main() -> i32 {
            return pick(true);
        }
    "#, "pick");
}

#[test]
fn test_if_without_else_can_reach_the_end() {
    assert_missing_return(r#"
        package main

        fn clamp(x: i32) -> i32 {
            if x > 10 {
                return 10;
            }
        }

        pub fn main() -> i32 {
            return clamp(5);
        }
    "#, "clamp");
}

#[test]
fn test_method_tail_expression_is_not_returned() {
    assert_missing_return(r#"
        package main

        struct Point { x: i32 }

        impl Point {
            fn get_x(&self) -> i32 {
                self.x
            }
        }

        pub fn main() -> i32 {
            let p = Point { x: 3 };
            return p.get_x();
        }
    "#, "get_x");
}

/// Instantiating a generic mid-body must not change which function the
/// error names.
#[test]
fn test_error_names_the_function_after_a_generic_call() {
    assert_missing_return(r#"
        package main

        fn id<T>(x: T) -> T {
            return x;
        }

        fn wrap(v: i32) -> i32 {
            let y = id(v);
        }

        pub fn main() -> i32 {
            return wrap(1);
        }
    "#, "wrap");
}

/// A `break` leaves the loop, so this `while true` can end.
#[test]
fn test_while_true_with_break_can_reach_the_end() {
    assert_missing_return(r#"
        package main

        fn first(n: i32) -> i32 {
            while true {
                if n > 0 { break; }
            }
        }

        pub fn main() -> i32 {
            return first(1);
        }
    "#, "first");
}

#[test]
fn test_loop_with_break_can_reach_the_end() {
    assert_missing_return(r#"
        package main

        fn first(n: i32) -> i32 {
            loop {
                if n > 0 { break; }
            }
        }

        pub fn main() -> i32 {
            return first(1);
        }
    "#, "first");
}

/// main returns 0 at its end, so a final value there would be dropped.
#[test]
fn test_main_final_expression_is_not_returned() {
    assert_missing_return(r#"
        package main

        fn half(x: i32) -> i32 {
            return x / 2;
        }

        pub fn main() -> i32 {
            half(10)
        }
    "#, "main");
}

/// The diagnostic says how to fix it.
#[test]
fn test_error_names_the_fix() {
    let msg = missing_return_error(r#"
        package main

        fn ten() -> i32 {
            10
        }

        pub fn main() -> i32 {
            return ten();
        }
    "#);
    assert!(msg.contains("return <value>;"), "{msg}");
}

#[test]
fn test_every_path_returns() {
    let code = r#"
        package main

        enum Shape {
            Circle(i32),
            Square(i32),
        }

        fn area(s: Shape) -> i32 {
            match s {
                Shape::Circle(r) => return 3 * r * r,
                Shape::Square(w) => return w * w,
            }
        }

        fn sign(x: i32) -> i32 {
            if x < 0 {
                return -1;
            } else if x == 0 {
                return 0;
            } else {
                return 1;
            }
        }

        pub fn main() -> i32 {
            return area(Shape::Square(2)) + sign(3);
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn test_main_still_returns_zero_at_its_end() {
    let code = r#"
        package main

        pub fn main() -> i32 {
            let x = 1;
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let body = fn_body(&mlir, "func.func public @main(");
    assert!(body.contains("func.return %c0_"), "{body}");
}

#[test]
fn test_unit_function_can_reach_its_end() {
    let code = r#"
        package main

        fn touch(x: i32) {
            let y = x + 1;
        }

        pub fn main() -> i32 {
            touch(1);
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "{:?}", result.err());
}

// Paths that end without `return` because control cannot continue past
// them: an infinite loop, or a call to C's `exit`/`abort` (std's
// Option::unwrap ends its None arm with `exit(1);`). A function returning a
// value may end in one; see missing_return_test.rs for the rule itself.

use saltc::compile;

/// The MLIR from the definition whose header contains `header` up to the
/// next function.
fn fn_body<'a>(mlir: &'a str, header: &str) -> &'a str {
    let start = mlir.find(header).unwrap_or_else(|| panic!("{header} not emitted:\n{mlir}"));
    let rest = &mlir[start..];
    rest[1..].find("func.func").map_or(rest, |end| &rest[..end + 1])
}

/// The trimmed line after the first line that `is_target` accepts.
fn line_after(body: &str, is_target: impl Fn(&str) -> bool) -> &str {
    let mut lines = body.lines().map(str::trim).skip_while(|l| !is_target(l));
    lines.next().unwrap_or_else(|| panic!("target line not found:\n{body}"));
    lines.next().unwrap_or("")
}

/// Whether a line of `body` that ends a block sits inside an `scf.for`,
/// whose single-block region may only end in its `scf.yield`.
fn has_terminator_in_scf_for(body: &str) -> bool {
    let mut depth_at_for: Vec<i32> = Vec::new();
    let mut depth = 0;
    for line in body.lines().map(str::trim) {
        if line.starts_with("scf.for ") {
            depth_at_for.push(depth);
        }
        let is_terminator = line == "llvm.unreachable" || line.starts_with("func.return");
        if is_terminator && !depth_at_for.is_empty() {
            return true;
        }
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        while depth_at_for.last().is_some_and(|d| depth <= *d) {
            depth_at_for.pop();
        }
    }
    false
}

fn assert_compiles(code: &str) {
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn test_infinite_loop_ends_function() {
    assert_compiles(r#"
        package main

        fn spin() -> i32 {
            loop {
            }
        }

        pub fn main() -> i32 {
            return spin();
        }
    "#);
}

/// `while true` with no `break` never exits, whatever its body returns.
#[test]
fn test_while_true_without_break_ends_function() {
    let code = r#"
        package main

        fn find(limit: i32) -> i32 {
            let mut i = 0;
            while true {
                if i == limit {
                    return i;
                }
                i = i + 1;
            }
        }

        pub fn main() -> i32 {
            return find(3);
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let body = fn_body(&mlir, "func.func private @main__find(");
    let after_exit = line_after(body, |l| l.starts_with("^while_exit_"));
    assert_eq!(after_exit, "llvm.unreachable", "{body}");
}

#[test]
fn test_parenthesized_while_true_ends_function() {
    assert_compiles(r#"
        package main

        fn find(limit: i32) -> i32 {
            let mut i = 0;
            while (true) {
                if i == limit { return i; }
                i = i + 1;
            }
        }

        pub fn main() -> i32 {
            return find(3);
        }
    "#);
}

/// A `break` inside `let ... else` also leaves the loop, so the code after
/// the loop is still emitted.
#[test]
fn test_while_true_with_let_else_break_keeps_trailing_return() {
    let code = r#"
        package main

        use std.core.option.*;

        fn next(i: i32) -> Option<i32> {
            if i < 3 {
                return Some(i);
            }
            return None;
        }

        fn count() -> i32 {
            let mut n = 0;
            while true {
                let Some(v) = next(n) else { break; };
                n = v + 1;
            }
            return n;
        }

        pub fn main() -> i32 {
            return count();
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let body = fn_body(&mlir, "func.func private @main__count(");
    assert!(body.contains("func.return"), "the return after the loop was dropped:\n{body}");
}

#[test]
fn test_exit_call_ends_function() {
    let code = r#"
        package main

        extern fn exit(code: i32);

        fn fail() -> i32 {
            exit(1);
        }

        pub fn main() -> i32 {
            return fail();
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let body = fn_body(&mlir, "func.func private @main__fail(");
    assert_eq!(line_after(body, |l| l.contains("@exit(")), "llvm.unreachable", "{body}");
}

#[test]
fn test_abort_call_ends_function() {
    let code = r#"
        package main

        extern fn abort();

        fn fail() -> i32 {
            abort();
        }

        pub fn main() -> i32 {
            return fail();
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let body = fn_body(&mlir, "func.func private @main__fail(");
    assert_eq!(line_after(body, |l| l.contains("@abort(")), "llvm.unreachable", "{body}");
}

/// The shape of std's Option::unwrap, and std's own.
#[test]
fn test_exit_in_match_arm_ends_function() {
    assert_compiles(r#"
        package main

        use std.core.option.*;

        extern fn exit(code: i32);

        fn get(o: Option<i32>) -> i32 {
            match o {
                Some(v) => return v,
                None => {
                    exit(1);
                }
            }
        }

        pub fn main() -> i32 {
            let a: Option<i32> = Some(4);
            let b: Option<i32> = Some(5);
            return get(a) + b.unwrap();
        }
    "#);
}

/// A loop body that calls `exit` ends in a terminator, so it cannot be
/// lowered into an scf.for region.
#[test]
fn test_exit_in_range_for_body_stays_out_of_scf_for() {
    for range in ["0..n", "0..3"] {
        let code = r#"
            package main

            extern fn exit(code: i32);
            extern fn putchar(c: i32) -> i32;

            fn run(n: i32) {
                for i in RANGE {
                    putchar(65);
                    exit(4);
                }
            }

            pub fn main() -> i32 {
                run(3);
                return 0;
            }
        "#.replace("RANGE", range);
        let mlir = compile(&code, false, None, true).expect("compiles");
        let body = fn_body(&mlir, "func.func private @main__run(");
        assert!(!has_terminator_in_scf_for(body), "{range}:\n{body}");
    }
}

/// Only C's `exit` never returns: a Salt function of the same name is an
/// ordinary call, whether or not the package mangles its name.
#[test]
fn test_user_function_named_exit_returns() {
    for package in ["package main", ""] {
        let code = r#"
            PACKAGE

            extern fn putchar(c: i32) -> i32;

            fn exit(depth: i32) -> i32 {
                putchar(88);
                return depth - 1;
            }

            fn walk(n: i32) -> i32 {
                let mut d = n;
                while d > 0 {
                    exit(d);
                    d = d - 1;
                }
                return 7;
            }

            fn main() -> i32 {
                return walk(2);
            }
        "#.replace("PACKAGE", package);
        let mlir = compile(&code, false, None, true).expect("compiles");
        let walk = if package.is_empty() { "@walk(" } else { "@main__walk(" };
        let body = fn_body(&mlir, &format!("func.func private {walk}"));
        assert!(!body.contains("llvm.unreachable"), "{package:?}:\n{body}");
    }
}

#[test]
fn test_user_function_named_abort_returns() {
    let code = r#"
        package main

        extern fn putchar(c: i32) -> i32;

        fn abort(code: i32) {
            putchar(65 + code);
        }

        fn check(x: i32) -> i32 {
            if x < 0 {
                abort(1);
            }
            return x + 100;
        }

        pub fn main() -> i32 {
            return check(0 - 5);
        }
    "#;
    let mlir = compile(code, false, None, true).expect("compiles");
    let body = fn_body(&mlir, "func.func private @main__check(");
    assert!(!body.contains("llvm.unreachable"), "{body}");
}

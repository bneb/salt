// Grammar coverage tests - exercising parsing paths

use saltc::compile;

// =============================================================================
// Parsing different constructs
// =============================================================================

#[test]
fn test_parse_public_fn() {
    let code = r#"
        pub fn public_func() -> i32 {
            return 42;
        }
        fn main() -> i32 {
            return public_func();
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Public fn failed: {:?}", result.err());
}

#[test]
fn test_parse_multiple_functions() {
    let code = r#"
        fn func1() -> i32 { return 1; }
        fn func2() -> i32 { return 2; }
        fn func3() -> i32 { return 3; }
        fn main() -> i32 {
            return func1() + func2() + func3();
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Multiple fns failed: {:?}", result.err());
}

#[test]
fn test_parse_attributes() {
    let code = r#"
        @hot
        fn hot_function() -> i32 {
            return 42;
        }
        fn main() -> i32 {
            return hot_function();
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Attribute parsing failed: {:?}", result.err());
}

#[test]
fn test_parse_multiple_structs() {
    let code = r#"
        struct Point { x: i32, y: i32 }
        struct Rect { width: i32, height: i32 }
        struct Circle { radius: i32 }
        fn main() -> i32 {
            let p: Point = Point { x: 0, y: 0 };
            let r: Rect = Rect { width: 10, height: 20 };
            let c: Circle = Circle { radius: 5 };
            return p.x + r.width + c.radius;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Multiple structs failed: {:?}", result.err());
}

#[test]
fn test_parse_generic_struct() {
    let code = r#"
        struct Container<T> {
            value: T
        }
        fn main() -> i32 {
            let c: Container<i32> = Container::<i32> { value: 42 };
            return c.value;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Generic struct failed: {:?}", result.err());
}

#[test]
fn test_parse_enum() {
    let code = r#"
        enum Status {
            Ok,
            Error
        }
        fn main() -> i32 {
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Enum failed: {:?}", result.err());
}

#[test]
fn test_parse_const() {
    let code = r#"
        const VALUE: i32 = 42;
        fn main() -> i32 {
            return VALUE;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Const failed: {:?}", result.err());
}

#[test]
fn test_parse_global() {
    let code = r#"
        global COUNTER: i32 = 0;
        fn main() -> i32 {
            return COUNTER;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Global failed: {:?}", result.err());
}

#[test]
fn test_parse_function_with_args() {
    let code = r#"
        fn add(a: i32, b: i32) -> i32 {
            return a + b;
        }
        fn main() -> i32 {
            return add(10, 20);
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Fn with args failed: {:?}", result.err());
}

#[test]
fn test_parse_nested_generics() {
    let code = r#"
        struct Box<T> { value: T }
        fn main() -> i32 {
            let b: Box<i32> = Box::<i32> { value: 42 };
            return b.value;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Nested generics failed: {:?}", result.err());
}

#[test]
fn test_parse_impl_with_method() {
    let code = r#"
        package test::impl_test;
        struct Point { x: i32, y: i32 }
        impl Point {
            fn sum(self: &Point) -> i32 {
                return self.x + self.y;
            }
        }
        fn main() -> i32 {
            let p: Point = Point { x: 10, y: 20 };
            return p.sum();
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Impl method failed: {:?}", result.err());
}

#[test]
fn test_parse_let_with_pattern() {
    let code = r#"
        fn main() -> i32 {
            let x: i32 = 42;
            let mut y: i32 = 10;
            y = y + 1;
            return x + y;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Let pattern failed: {:?}", result.err());
}

#[test]
fn test_parse_empty_function() {
    let code = r#"
        fn empty() {
        }
        fn main() -> i32 {
            empty();
            return 0;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Empty function failed: {:?}", result.err());
}

#[test]
fn test_parse_usize_type() {
    let code = r#"
        fn main() -> i32 {
            let x: usize = 100 as usize;
            return x as i32;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Usize type failed: {:?}", result.err());
}

#[test]
fn test_parse_float_types() {
    let code = r#"
        fn main() -> i32 {
            let f32_val: f32 = 3.14 as f32;
            let f64_val: f64 = 2.71828;
            return f32_val as i32;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Float types failed: {:?}", result.err());
}

#[test]
fn test_parse_hex_literal() {
    let code = r#"
        fn main() -> i32 {
            let x: i32 = 0xFF00;
            return x;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Hex literal failed: {:?}", result.err());
}

#[test]
fn test_parse_binary_literal() {
    let code = r#"
        fn main() -> i32 {
            let x: i32 = 0b10101010;
            return x;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "Binary literal failed: {:?}", result.err());
}

// ============================================================================
// Grammar extensions: static/comptime fn markers, var locals,
// hash-bracket attributes, and defaulted trait methods.
// ============================================================================

#[test]
fn test_parse_static_fn_in_impl() {
    let code = r#"
        struct Rng { state0: u64, state1: u64 }
        impl Rng {
            static fn new(seed: u64) -> Rng {
                return Rng { state0: seed, state1: seed ^ 1 };
            }
            fn mix(&mut self) -> u64 {
                return self.state0;
            }
        }
        fn main() -> i32 { return 0; }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "static fn in impl failed: {:?}", result.err());
}

#[test]
fn test_parse_comptime_fn_marker() {
    let code = r#"
        pub comptime fn hex_handler(x: i64) -> i64 {
            return x + 1;
        }
        fn main() -> i32 { return 0; }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "comptime fn marker failed: {:?}", result.err());
}

#[test]
fn test_parse_var_local_typed_and_untyped() {
    let code = r#"
        fn main() -> i64 {
            var total: i64 = 0;
            var step = 3;
            total = total + step;
            return total;
        }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "var locals failed: {:?}", result.err());
}

#[test]
fn test_parse_hash_bracket_attribute() {
    let code = r#"
        #[string_prefix("zz")]
        fn handler(x: i64) -> i64 {
            return x;
        }
        fn main() -> i32 { return 0; }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "hash-bracket attribute failed: {:?}", result.err());
}

#[test]
fn test_parse_trait_default_method_body() {
    let code = r#"
        trait Shape {
            fn area(&self) -> f64;
            fn describe(&self) -> f64 {
                return self.area() * 2.0;
            }
        }
        fn main() -> i32 { return 0; }
    "#;
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "trait default body failed: {:?}", result.err());
}

#[test]
fn test_trait_method_needs_semicolon_or_body() {
    // Bodyless method without trailing semicolon must be rejected.
    let code = "trait Bad { fn oops(&self) -> i64 }";
    let result = compile(code, false, None, true);
    assert!(result.is_err(), "trait member without ; or body must fail");
}

#[test]
fn test_var_without_initializer_rejected() {
    let code = "fn f() { var x; }";
    let result = compile(code, false, None, true);
    assert!(result.is_err(), "var requires an initializer");
}

#[test]
fn test_static_marker_requires_fn() {
    let code = "struct S { v: i64 } impl S { static }";
    let result = compile(code, false, None, true);
    assert!(result.is_err(), "stray 'static' inside impl must fail");
}

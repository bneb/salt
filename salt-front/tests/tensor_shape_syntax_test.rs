// Regression: TENSOR SHAPE SYNTAX in type positions (item #5).
//
// preprocess() rewrites `Tensor<f32, {1, D}>` into the marker form
// `Tensor<f32, __Shape_2_1_D__>`, but grammar.rs's Tensor arm still
// demanded the braced form — so every rewritten shape (struct fields,
// annotations) failed with "expected curly braces". The arm now accepts
// the marker ident and reconstructs rank/dims from it.
use saltc::compile;

#[test]
fn symbolic_const_generic_shape_field_parses() {
    let src = r#"
        package main

        struct Layer<const D: i64> {
            w: Tensor<f32, {1, D}>,
        }

        pub fn main() -> i32 {
            return 0;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "const-generic tensor shape failed: {:?}", result.err());
}

#[test]
fn static_shape_still_parses() {
    let src = r#"
        package main

        struct Net {
            w: Tensor<f32, {128, 784}>,
        }

        pub fn main() -> i32 {
            return 0;
        }
    "#;
    let result = compile(src, false, None, true);
    assert!(result.is_ok(), "static tensor shape failed: {:?}", result.err());
}

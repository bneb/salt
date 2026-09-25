//! S2.3b: Typed instance lookup verification tests.
//!
//! Tests that struct lookups resolve specialized generic instances via
//! InstanceId and the typed side-map registered during define_struct_instance.

use saltc::compile;

#[test]
fn test_generic_struct_instance_field_lookup() {
    const SRC: &str = r#"
        package main

        struct Pair<T, U> {
            first: T,
            second: U,
        }

        impl<T, U> Pair<T, U> {
            pub fn new(a: T, b: U) -> Pair<T, U> {
                return Pair { first: a, second: b };
            }

            pub fn get_first(self) -> T {
                return self.first;
            }
        }

        pub fn main() -> i32 {
            let p = Pair::<i32, i64>::new(42, 100);
            return p.get_first();
        }
    "#;

    let mlir = compile(SRC, false, None, true)
        .expect("Pair<i32, i64> instance lookup and field access must compile");
    assert!(mlir.contains("Pair_i32_i64"), "specialized struct must be emitted");
    assert!(mlir.contains("get_first"), "getter must be emitted");
}

#[test]
fn test_multiple_generic_struct_instances_coexist() {
    const SRC: &str = r#"
        package main

        struct Box<T> {
            val: T,
        }

        impl<T> Box<T> {
            pub fn make(v: T) -> Box<T> {
                return Box { val: v };
            }
            pub fn unwrap(self) -> T {
                return self.val;
            }
        }

        pub fn main() -> i32 {
            let b1 = Box::<i32>::make(10);
            let b2 = Box::<i64>::make(20);
            return b1.unwrap() + (b2.unwrap() as i32);
        }
    "#;

    let mlir = compile(SRC, false, None, true)
        .expect("multiple Box instances must resolve independently");
    assert!(mlir.contains("Box_i32"), "Box_i32 must be emitted");
    assert!(mlir.contains("Box_i64"), "Box_i64 must be emitted");
}


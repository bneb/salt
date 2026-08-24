// Regression tests: method-call receiver resolution patterns behind the
// std.args / std.http.client / std.fmt.display / std.core.conv fixes.
//
// Two root causes are locked here:
//
// 1. Chained method calls (`x.m().n()`) - the receiver of `.n()` is itself a
//    method call. get_receiver_lvalue used to re-run emit_lvalue on such
//    receivers, which fully emitted the inner call a second time before
//    failing on its non-reference return type (duplicated side effects),
//    and produced the misleading "Method call 'n' requires a receiver value"
//    error whenever the inner call failed to resolve.
//
// 2. Missing accessors the modules were written against: StringView had no
//    data() (std.args, std.http.client used it for FFI hand-off), Ptr<T>
//    exposes read/write (not load/store - std.core.conv), and std.fmt used
//    a nonexistent String::as_bytes().get(i).
use saltc::compile;

fn assert_compiles(code: &str, label: &str) {
    let result = compile(code, false, None, true);
    assert!(result.is_ok(), "{} failed: {:?}", label, result.err());
}

/// std.core.conv pattern: store through a GEP'd pointer returned by .offset().
#[test]
fn test_chained_ptr_offset_write() {
    let code = r#"
        package main

        use std.core.ptr.Ptr;

        @trusted
        fn poke(buf: Ptr<u8>, idx: i64, val: u8) {
            buf.offset(idx).write(val);
        }

        pub fn main() -> i32 {
            return 0;
        }
    "#;
    assert_compiles(code, "chained Ptr::offset().write()");
}

/// std.core.conv pattern: load through a GEP'd pointer returned by .offset().
#[test]
fn test_chained_ptr_offset_read() {
    let code = r#"
        package main

        use std.core.ptr.Ptr;

        @trusted
        fn peek(buf: Ptr<u8>, idx: i64) -> u8 {
            return buf.offset(idx).read();
        }

        pub fn main() -> i32 {
            return 0;
        }
    "#;
    assert_compiles(code, "chained Ptr::offset().read()");
}

/// std.args pattern: a.data().read() - receiver of .read() is a method result.
#[test]
fn test_stringview_data_then_read_chain() {
    let code = r#"
        package main

        use std.core.str.StringView;

        @trusted
        fn first_byte(s: StringView) -> u8 {
            return s.data().read();
        }

        pub fn main() -> i32 {
            return 0;
        }
    "#;
    assert_compiles(code, "StringView::data().read() chain");
}

/// std.http.client pattern: sv.data() handed straight to an extern FFI fn.
#[test]
fn test_stringview_data_ffi_passthrough() {
    let code = r#"
        package main

        use std.core.str.StringView;
        use std.core.ptr.Ptr;

        extern fn bridge_take_bytes(p: Ptr<u8>) -> i32;

        @trusted
        fn send(s: StringView) -> i32 {
            return bridge_take_bytes(s.data());
        }

        pub fn main() -> i32 {
            return 0;
        }
    "#;
    assert_compiles(code, "StringView::data() FFI passthrough");
}

/// std.fmt.display pattern: copy bytes out of a String via as_ptr() reads.
#[test]
fn test_string_bytes_via_as_ptr_read_loop() {
    let code = r#"
        package main

        use std.string.String;

        fn copy_digits(dst: &mut String) {
            let mut temp = String::new();
            temp.push_ascii(48);
            let len = temp.len();
            let bytes = temp.as_ptr();
            for i in 0..len {
                dst.push_byte(bytes.offset(i).read());
            }
        }

        pub fn main() -> i32 {
            let mut s = String::new();
            copy_digits(&mut s);
            return 0;
        }
    "#;
    assert_compiles(code, "String byte copy via as_ptr().offset().read()");
}

/// The four originally failing std modules compile standalone with the exact
/// gate invocation: saltc <file> --lib --disable-alias-scopes -o /dev/null
fn assert_std_module_compiles(rel_path: &str) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_saltc"))
        .current_dir(manifest_dir)
        .args([rel_path, "--lib", "--disable-alias-scopes", "-o", "/dev/null"])
        .output()
        .expect("failed to spawn saltc binary");
    assert!(
        output.status.success(),
        "saltc --lib failed for {}:\nstdout:\n{}\nstderr:\n{}",
        rel_path,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_std_args_module_compiles_standalone() {
    assert_std_module_compiles("std/args/args.salt");
}

#[test]
fn test_std_http_client_module_compiles_standalone() {
    assert_std_module_compiles("std/http/client.salt");
}

#[test]
fn test_std_fmt_display_module_compiles_standalone() {
    assert_std_module_compiles("std/fmt/display.salt");
}

#[test]
fn test_std_core_conv_module_compiles_standalone() {
    assert_std_module_compiles("std/core/conv.salt");
}
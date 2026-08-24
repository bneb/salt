// Guards for std modules that previously failed to build standalone.
//
// Two tiers of protection:
//
// 1. Parse tier (`*_parses`): preprocess + syn parse. Locks in grammar
//    features these modules exercise (static/comptime fn markers, typed
//    var locals, hash-bracket attributes, defaulted trait methods,
//    concept-free traits).
//
// 2. Compile tier (`*_compiles`): full pipeline against the embedded
//    stdlib bundle — imports resolved, codegen lowered, Z3 contracts
//    verified, lib mode. Mirrors the release audit:
//      saltc <file> --lib --disable-alias-scopes -o out.mlir
//    These cover the nine modules healed in the keuos/io round:
//    arena (prefix-`~` bitwise-not), context/mailbox/executor (qualified
//    intrinsic paths, `null` literals, unsafe-expression positions),
//    their sovereign twins, and std.io (unprovable syscall counts).

use saltc::grammar::SaltFile;
use saltc::preprocess;

/// Parse a real std source file through the same pipeline as the CLI:
/// preprocess to syn-friendly text, then parse into the Salt AST.
fn assert_module_parses(rel_path: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel_path);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", rel_path, e));
    let processed = preprocess(&src);
    if let Err(e) = syn::parse_str::<SaltFile>(&processed) {
        panic!("{} failed to parse: {}", rel_path, e);
    }
}

/// Full standalone-library compilation of a real std module, matching the
/// release audit invocation. Imports resolve through the embedded bundle.
fn assert_module_compiles(rel_path: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel_path);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", rel_path, e));
    let processed = preprocess(&src);
    let mut file: SaltFile = syn::parse_str(&processed)
        .unwrap_or_else(|e| panic!("{} failed to parse: {}", rel_path, e));

    let mut registry = saltc::registry::Registry::new();
    let main_pkg = file
        .package
        .as_ref()
        .map(|p| p.name.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("."))
        .unwrap_or_else(|| "main".to_string());
    registry.register(saltc::registry::ModuleInfo::new(&main_pkg));

    // Mirror src/cli.rs: the compiler prelude wires in Ptr/Option/Result and
    // friends so method registries (e.g. Ptr::offset) resolve for modules
    // that use them without an explicit import.
    let prelude_imports = [
        "use std::core::ptr::Ptr;",
        "use std::core::option::Option;",
        "use std::core::result::Result;",
        "use std::status::Status;",
        "use std::arena::default::DefaultAllocator;",
        "use std::io::print::*;",
    ];
    for import_str in &prelude_imports {
        let processed = preprocess(import_str);
        if let Ok(parsed) = syn::parse_str::<SaltFile>(&processed) {
            file.imports.extend(parsed.imports);
        }
    }

    saltc::cli::load_imports(&file, &mut registry, None);

    let result = saltc::compile_ast(
        &mut file, false, Some(&registry),
        false, // skip_scan: resolve imports like the CLI does
        true,  // disable_alias_scopes: match audit flags
        false, // no_verify: keep Z3 contract checking ON
        true,  // lib_mode
        false, false, // sip_mode, debug_info
        false, // deny_deferred
        rel_path,
    );
    assert!(result.is_ok(), "{} failed to compile: {:?}", rel_path, result.err());
}

#[test]
fn std_random_mod_parses() {
    assert_module_parses("std/random/mod.salt");
}

#[test]
fn std_io_writer_parses() {
    assert_module_parses("std/io/writer.salt");
}

#[test]
fn std_syscalls_parses() {
    assert_module_parses("std/syscalls.salt");
}

#[test]
fn std_comptime_parses() {
    assert_module_parses("std/comptime.salt");
}

#[test]
fn std_core_alloc_parses() {
    assert_module_parses("std/core/alloc.salt");
}

// ===========================================================================
// Compile-tier guards: the nine modules healed in the keuos/io round.
// ===========================================================================

#[test]
fn std_keuos_arena_compiles() {
    assert_module_compiles("std/core/keuos/arena.salt");
}

#[test]
fn std_keuos_context_compiles() {
    assert_module_compiles("std/core/keuos/context.salt");
}

#[test]
fn std_keuos_executor_compiles() {
    assert_module_compiles("std/core/keuos/executor.salt");
}

#[test]
fn std_keuos_mailbox_compiles() {
    assert_module_compiles("std/core/keuos/mailbox.salt");
}

#[test]
fn std_keuos_sovereign_arena_compiles() {
    assert_module_compiles("std/core/keuos/sovereign/arena.salt");
}

#[test]
fn std_keuos_sovereign_context_compiles() {
    assert_module_compiles("std/core/keuos/sovereign/context.salt");
}

#[test]
fn std_keuos_sovereign_executor_compiles() {
    assert_module_compiles("std/core/keuos/sovereign/executor.salt");
}

#[test]
fn std_keuos_sovereign_mailbox_compiles() {
    assert_module_compiles("std/core/keuos/sovereign/mailbox.salt");
}

#[test]
fn std_io_mod_compiles() {
    assert_module_compiles("std/io/mod.salt");
}

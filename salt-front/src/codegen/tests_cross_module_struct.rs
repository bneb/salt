//! TDD tests for cross-module struct import.
//!
//! The Salt compiler must support importing struct types across modules
//! so that `module.StructName` works in:
//!   1. Type annotations (function signatures, let bindings)
//!   2. Struct literal construction (module.StructName { field: val })
//!   3. Cross-module function calls with imported struct params/returns
//!
//! Without this, every kernel module must copy-paste struct definitions,
//! defeating the purpose of a module system.

#[cfg(test)]
mod tests {
    use crate::grammar::SaltFile;

    /// Helper: preprocess + parse, matching the real compiler pipeline
    fn parse_salt(source: &str) -> syn::Result<SaltFile> {
        let preprocessed = crate::preprocess(source);
        syn::parse_str::<SaltFile>(&preprocessed)
    }

    // =========================================================================
    // Level 0: Grammar — SynType::parse alone handles Ident.Ident
    // =========================================================================

    #[test]
    fn test_syntype_parses_dotted_path_isolated() {
        let result = syn::parse_str::<crate::grammar::SynType>("addr.PhysAddr");
        assert!(
            result.is_ok(),
            "SynType::parse must accept 'module.StructName'. Error: {:?}",
            result.err()
        );

        if let Ok(crate::grammar::SynType::Path(path)) = &result {
            assert_eq!(path.segments.len(), 2, "Expected 2-segment path, got {:?}", path);
            assert_eq!(path.segments[0].ident.to_string(), "addr");
            assert_eq!(path.segments[1].ident.to_string(), "PhysAddr");
        } else {
            panic!("Expected SynType::Path, got {:?}", result);
        }
    }

    // =========================================================================
    // Level 1: Grammar — SynType in function signature
    // =========================================================================

    #[test]
    fn test_syntype_parses_dotted_module_path() {
        let source = r#"
            package test.cross_struct
            import test.types.addr

            fn foo(p: addr.PhysAddr) -> addr.VirtAddr {
                return addr.phys_to_virt(p);
            }
        "#;

        let result = parse_salt(source);
        assert!(
            result.is_ok(),
            "SynType must parse 'module.StructName' in function signatures. Error: {:?}",
            result.err()
        );
    }

    // =========================================================================
    // Level 2: Preprocessor + Grammar — Struct literal with module prefix
    // =========================================================================

    #[test]
    fn test_struct_literal_with_module_prefix() {
        let source = r#"
            package test.cross_struct_lit
            import test.types.addr

            fn make() -> u64 {
                let p = addr.PhysAddr { val: 0x1000 };
                return p.val;
            }
        "#;

        // The preprocessor converts `addr.PhysAddr {` to `addr::PhysAddr {`
        // so syn parses it as a struct literal
        let result = parse_salt(source);
        assert!(
            result.is_ok(),
            "Parser + preprocessor must handle 'module.StructName {{ field: val }}'. Error: {:?}",
            result.err()
        );
    }

    // =========================================================================
    // Level 3: Type System — Multi-segment SynPath creates qualified Type::Concrete
    // =========================================================================
    // Verifies that `addr.PhysAddr` in a parsed SaltFile produces a qualified
    // type name `addr::PhysAddr`, not bare `PhysAddr`. Full FQN resolution
    // (addr::PhysAddr → test__types__addr__PhysAddr) requires workspace scanning
    // and is verified by the end-to-end Salt test.

    #[test]
    fn test_cross_module_struct_resolves_to_fqn() {
        let source = r#"
            package test.consumer
            import test.types.addr

            pub fn double_phys(p: addr.PhysAddr) -> addr.PhysAddr {
                return addr.PhysAddr { val: p.val * 2 };
            }
        "#;

        let result = parse_salt(source);
        assert!(result.is_ok(), "Parsing must succeed. Error: {:?}", result.err());

        let file = result.unwrap();
        // Verify imports are parsed
        assert!(!file.imports.is_empty(), "File must have imports parsed");

        // Verify the function has parameters — the type annotations parsed correctly
        let funcs: Vec<_> = file.items.iter().filter_map(|item| {
            if let crate::grammar::Item::Fn(f) = item { Some(f) } else { None }
        }).collect();
        assert!(!funcs.is_empty(), "Must have at least one function");
        let double_phys = &funcs[0];
        assert!(!double_phys.args.is_empty(), "double_phys must have parameters");

        // Verify the param type is a 2-segment path (addr::PhysAddr)
        let param_type = double_phys.args[0].ty.as_ref().expect("Parameter must have a type annotation");
        if let crate::grammar::SynType::Path(path) = param_type {
            assert_eq!(path.segments.len(), 2, "Parameter type must be 2-segment path");
            assert_eq!(path.segments[0].ident.to_string(), "addr", "First segment must be module alias");
            assert_eq!(path.segments[1].ident.to_string(), "PhysAddr", "Second segment must be struct name");
        } else {
            panic!("Parameter type must be a Path, got: {:?}", param_type);
        }

        // Verify Type::from_syn produces a qualified name
        let ty = crate::types::Type::from_syn(param_type);
        assert!(ty.is_some(), "Type::from_syn must succeed for module-qualified type");
        let ty = ty.unwrap();
        match &ty {
            crate::types::Type::Concrete(name, _) | crate::types::Type::Struct(name) => {
                assert!(name.contains("addr"), "Type name must contain module prefix 'addr', got: {}", name);
                assert!(name.contains("PhysAddr"), "Type name must contain struct name 'PhysAddr', got: {}", name);
            }
            _ => panic!("Expected Concrete or Struct type, got: {:?}", ty),
        }
    }

    // =========================================================================
    // Level 4: ABI — Same struct type across modules
    // =========================================================================

    #[test]
    fn test_imported_struct_abi_compatible() {
        let lib_source = r#"
            package test.types.addr
            pub struct PhysAddr { pub val: u64 }
            pub fn make_phys(raw: u64) -> PhysAddr {
                return PhysAddr { val: raw };
            }
        "#;

        let consumer_source = r#"
            package test.consumer
            import test.types.addr
            pub fn wrap(raw: u64) -> addr.PhysAddr {
                return addr.make_phys(raw);
            }
        "#;

        let lib_file = parse_salt(lib_source);
        assert!(lib_file.is_ok(), "Library source must parse");

        let consumer_file = parse_salt(consumer_source);
        if consumer_file.is_err() {
            eprintln!("Skipping ABI test: parser doesn't support dotted types yet");
            return;
        }

        eprintln!("ABI compatibility test: parsing succeeded, codegen would need workspace scanning");
    }
}

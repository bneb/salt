//! Conventions mirrored from types/generic_arg_tests.rs (sibling *_tests.rs,
//! `#[cfg(test)] mod tests`) and type_bridge.rs tests (CodegenContext::new +
//! `with_lowering_ctx` yields &mut LoweringContext without RefCell juggling).
#[cfg(test)]
mod tests {
    use crate::codegen::context::{CodegenContext, LoweringContext};
    use crate::grammar::SaltFile;
    use crate::registry::{EnumInfo, StructInfo};
    use crate::types::{Type, TypeKey};
    use std::collections::HashMap;

    fn lctx<R>(f: impl FnOnce(&mut LoweringContext<'_, '_>) -> R) -> R {
        let file: SaltFile = syn::parse_str("fn main() {}").unwrap();
        let z3_cfg = crate::z3_shim::Config::new();
        let z3_ctx = crate::z3_shim::Context::new(&z3_cfg);
        // Bind the context: the temporary would drop before `f`'s result
        // is proven independent of the LoweringContext borrow.
        let ctx = CodegenContext::new(&file, false, None, &z3_ctx);
        ctx.with_lowering_ctx(f)
    }

    fn key(name: &str) -> TypeKey {
        TypeKey { path: vec![], name: name.to_string(), specialization: None }
    }

    fn placeholder(args: Vec<Type>) -> StructInfo {
        StructInfo {
            name: "main__Pair_T".into(), fields: HashMap::new(),
            field_order: Vec::new(), field_alignments: Vec::new(),
            template_name: (!args.is_empty()).then(|| "main__Pair".into()),
            specialization_args: args,
        }
    }

    #[test]
    fn struct_define_routes_into_struct_registry_only() {
        lctx(|ctx| {
            ctx.define_struct_instance(key("main__Pair_T"), placeholder(vec![Type::U8]));
            let got = ctx.struct_registry().get(&key("main__Pair_T")).expect("routed");
            assert_eq!(got.name, "main__Pair_T");
            assert_eq!(got.template_name.as_deref(), Some("main__Pair"));
            assert_eq!(got.specialization_args, vec![Type::U8]);
            assert!(ctx.enum_registry().is_empty());
        });
    }

    #[test]
    fn enum_define_routes_into_enum_registry_only() {
        lctx(|ctx| {
            let info = EnumInfo {
                name: "main__Opt_T".into(), variants: vec![], max_payload_size: 0,
                template_name: Some("main__Opt".into()),
                specialization_args: vec![Type::I64],
            };
            ctx.define_enum_instance(key("main__Opt_T"), info);
            assert!(ctx.struct_registry().is_empty());
            assert!(ctx.enum_registry().contains_key(&key("main__Opt_T")));
        });
    }

    #[test]
    fn redefine_displaces_placeholder_and_emits_nothing() {
        // Mirrors specialize_template: step-6 placeholder insert then
        // expansion-result overwrite (spec_template.rs:327-341, :353/:374).
        lctx(|ctx| {
            ctx.define_struct_instance(key("k"), placeholder(vec![]));
            let expanded = StructInfo {
                name: "k".into(), fields: HashMap::new(),
                field_order: vec![Type::U8], field_alignments: vec![None],
                template_name: None, specialization_args: vec![Type::U8],
            };
            ctx.define_struct_instance(key("k"), expanded);
            assert_eq!(ctx.struct_registry().get(&key("k")).unwrap().field_order, vec![Type::U8]);
            assert!(ctx.decl_out().is_empty());
            assert!(ctx.definitions_buffer().is_empty());
        });
    }
}

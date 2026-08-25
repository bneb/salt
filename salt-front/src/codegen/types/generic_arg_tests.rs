#[cfg(test)]
mod tests {
    use crate::codegen::types::generic_arg::{GenericArg, ParamId, ParamOwner};
    use crate::evaluator::ConstValue;
    use crate::grammar::GenericParam;
    use crate::types::Type;
    use std::cmp::Ordering;
    use std::collections::hash_map::DefaultHasher;
    use std::collections::BTreeMap;
    use std::hash::{Hash, Hasher};

    fn legacy(cv: ConstValue) -> Type {
        GenericArg::from_const_value(cv).to_legacy_type()
    }

    #[test]
    fn test_integer_consts_wrap_as_struct_decimals() {
        // Mirrors seeker.rs:420, scan_types.rs:146, expr/resolver.rs:101,248.
        for i in [0i64, 1, 64, -7, i64::MAX] {
            assert_eq!(legacy(ConstValue::Integer(i)), Type::Struct(i.to_string()));
        }
    }

    #[test]
    fn test_non_integer_consts_fall_back_to_zero() {
        // Mirrors the scan_types.rs:147,149 Struct("0") fallback.
        let values = [
            ConstValue::Float(2.5),
            ConstValue::Bool(true),
            ConstValue::String("x".into()),
            ConstValue::Array(vec![ConstValue::Integer(1)]),
            ConstValue::Complex,
        ];
        for value in values {
            assert_eq!(legacy(value), Type::Struct("0".into()));
        }
    }

    #[test]
    fn test_type_args_round_trip_identity() {
        let ty = Type::Concrete("Vec".into(), vec![Type::Generic("T".into())]);
        let arg = GenericArg::from_type(ty.clone());
        assert_eq!(arg.as_type(), Some(&ty));
        assert_eq!(arg.as_const(), None);
        assert_eq!(arg.to_legacy_type(), ty);
    }

    #[test]
    fn test_legacy_spelling_round_trip_property() {
        for i in [-3i64, 0, 64, 255] {
            match legacy(ConstValue::Integer(i)) {
                Type::Struct(spelling) => assert_eq!(
                    GenericArg::from_legacy_struct_value(&spelling, false),
                    Ok(GenericArg::Const(ConstValue::Integer(i)))
                ),
                other => panic!("expected Struct spelling, got {other:?}"),
            }
        }
        assert_eq!(
            GenericArg::from_legacy_struct_value("2.5", true),
            Ok(GenericArg::Type(Type::Struct("2.5".into())))
        );
        // RT4 amendment: f64 spellings (incl. "inf"/"nan") must never be
        // swallowed as consts — they are legal Salt identifiers.
        assert!(GenericArg::from_legacy_struct_value("inf", false).is_err());
        assert!(GenericArg::from_legacy_struct_value("nan", false).is_err());
        assert_eq!(
            GenericArg::from_legacy_struct_value("inf", true),
            Ok(GenericArg::Type(Type::Struct("inf".into())))
        );
    }

    #[test]
    fn test_non_numeric_rejected_unless_allowed() {
        assert!(GenericArg::from_legacy_struct_value("SIZE", false).is_err());
        assert_eq!(
            GenericArg::from_legacy_struct_value("SIZE", true),
            Ok(GenericArg::Type(Type::Struct("SIZE".into())))
        );
    }

    #[test]
    fn test_zero_and_nan_float_identities() {
        assert_eq!(
            GenericArg::Const(ConstValue::Float(0.0)),
            GenericArg::Const(ConstValue::Float(-0.0))
        );
        let nan = GenericArg::Const(ConstValue::Float(f64::NAN));
        assert_eq!(nan, nan);
        assert_ne!(nan, GenericArg::Const(ConstValue::Float(1.5)));
    }

    fn hash_of(arg: &GenericArg) -> u64 {
        let mut hasher = DefaultHasher::new();
        arg.hash(&mut hasher);
        hasher.finish()
    }

    /// Property: Eq <=> !distinct-Ord, eq implies same hash, cmp antisymmetric.
    fn assert_pair(a: &GenericArg, b: &GenericArg) {
        if a == b {
            assert_eq!(hash_of(a), hash_of(b), "eq must imply same hash");
        } else {
            assert_ne!(a.cmp(b), Ordering::Equal);
        }
        assert_eq!(a.cmp(b), b.cmp(a).reverse());
    }

    #[test]
    fn test_eq_hash_ord_consistency_property() {
        let samples = vec![
            GenericArg::Type(Type::I64),
            GenericArg::Type(Type::Struct("64".into())),
            GenericArg::Const(ConstValue::Integer(64)),
            GenericArg::Const(ConstValue::Integer(-1)),
            GenericArg::Const(ConstValue::Float(1.5)),
            GenericArg::Const(ConstValue::Bool(true)),
            GenericArg::Const(ConstValue::String("a".into())),
            GenericArg::Const(ConstValue::Array(vec![])),
            GenericArg::Const(ConstValue::Array(vec![ConstValue::Integer(1)])),
            GenericArg::Const(ConstValue::Complex),
        ];
        let n = samples.len();
        let pairs = (0..n).flat_map(|i| (0..n).map(move |j| (i, j)));
        for (i, j) in pairs {
            assert_pair(&samples[i], &samples[j]);
        }
        let mut sorted = samples.clone();
        sorted.sort();
        sorted.dedup_by(|a, b| a == b);
        let mut map: BTreeMap<&GenericArg, u8> = BTreeMap::new();
        for sample in &sorted {
            *map.entry(sample).or_insert(0) += 1;
        }
        assert_eq!(map.len(), sorted.len(), "BTreeMap keys must dedupe like Eq");
    }

    #[test]
    fn test_param_id_identity_ignores_display_name() {
        let id = ParamId::from_grammar_param("main__Cache", &type_param("NODE"), 1);
        assert_eq!(id.owner(), &ParamOwner::mangled("main__Cache"));
        assert_eq!(id.index(), 1);
        assert_eq!(id.to_string(), "main__Cache::NODE#1");
        let renamed = ParamId::from_grammar_param("main__Cache", &type_param("WIDTH"), 1);
        assert_eq!(id, renamed, "identity is (owner, index); name is cosmetic");
        assert_ne!(id, ParamId::from_grammar_param("main__Cache", &type_param("NODE"), 2));
    }

    fn type_param(name: &str) -> GenericParam {
        GenericParam::Type {
            name: proc_macro2::Ident::new(name, proc_macro2::Span::call_site()),
            constraint: None,
        }
    }
}

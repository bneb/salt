//! S1 golden pins for the typed instance identity (design D1). The pin:
//! `InstanceId::to_legacy_type_key()` must be byte-equal to constructing
//! today's TypeKey directly from legacy spellings -- proving S1 is
//! emission-neutral before any caller migrates in S2+.
#[cfg(test)]
mod tests {
    use crate::codegen::types::generic_arg::{GenericArg, ParamOwner};
    use crate::codegen::types::instance_id::{FnInstanceId, InstanceId};
    use crate::evaluator::ConstValue;
    use crate::types::{Type, TypeKey};

    fn owner() -> ParamOwner { ParamOwner::mangled("main__S") }

    fn id(args: Vec<GenericArg>) -> InstanceId {
        InstanceId::from_parts(owner(), args).expect("valid args")
    }

    #[test]
    fn legacy_type_key_pin_matches_direct_construction() {
        let args = vec![
            GenericArg::Const(ConstValue::Integer(-7)),
            GenericArg::Type(Type::I64),
        ];
        let key = id(args.clone()).to_legacy_type_key();
        let direct = TypeKey {
            path: vec![],
            name: "main__S".into(),
            specialization: Some(vec![Type::Struct("-7".into()), Type::I64]),
        };
        assert_eq!(key.mangle(), direct.mangle());
        assert_eq!(key, direct);
    }

    #[test]
    fn empty_args_yield_specialization_none() {
        let key = id(vec![]).to_legacy_type_key();
        assert!(key.specialization.is_none());
        assert_eq!(key.mangle(), "main__S");
    }

    #[test]
    fn refusing_constructor_rejects_value_spellings() {
        let rejected = vec![
            GenericArg::Type(Type::Struct("-7".into())),
            GenericArg::Type(Type::Struct("64".into())),
            GenericArg::Type(Type::Struct("true".into())),
        ];
        for arg in rejected {
            assert!(
                InstanceId::from_parts(owner(), vec![arg]).is_err(),
                "value spelling must refuse"
            );
        }
    }

    #[test]
    fn fn_instance_carries_validated_args() {
        let fid = FnInstanceId::from_parts(
            owner(),
            "mk".into(),
            vec![GenericArg::Const(ConstValue::Integer(64))],
        )
        .expect("integer consts are valid");
        assert_eq!(fid.member(), "mk");
    }
}

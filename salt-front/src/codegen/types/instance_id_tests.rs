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
    fn from_legacy_classifies_digit_and_keyword_leaves() {
        use crate::codegen::types::instance_id::InstanceId;
        let id = InstanceId::from_legacy(owner(), &[
            Type::Struct("-7".into()),
            Type::Struct("64".into()),
            Type::Struct("main__Node".into()),
            Type::Struct("4612811918334230528".into()), // positive f64 bits
        ]);
        assert_eq!(
            id.args(),
            &[
                GenericArg::Const(ConstValue::Integer(-7)),
                GenericArg::Const(ConstValue::Integer(64)),
                GenericArg::Type(Type::Struct("main__Node".into())),
                // Fits i64 => Const; NEGATIVE float bits (>= 2^63) would
                // fail the strict parse and stay honest Type spellings.
                GenericArg::Const(ConstValue::Integer(4612811918334230528)),
            ]
        );
    }

    #[test]
    fn from_legacy_key_round_trips_byte_identical() {
        use crate::codegen::types::instance_id::InstanceId;
        let legacy = vec![Type::Struct("-7".into())];
        let id = InstanceId::from_legacy(owner(), &legacy);
        let direct = TypeKey {
            path: vec![],
            name: "main__S".into(),
            specialization: Some(legacy.clone()),
        };
        assert_eq!(id.to_legacy_type_key().mangle(), direct.mangle());
    }

    #[test]
    fn keyword_leaves_carry_bool_kind_after_s3() {
        use crate::codegen::types::instance_id::InstanceId;
        let id = InstanceId::from_legacy(owner(), &[
            Type::Struct("true".into()),
            Type::Struct("false".into()),
        ]);
        assert_eq!(
            id.args(),
            &[
                GenericArg::Const(ConstValue::Bool(true)),
                GenericArg::Const(ConstValue::Bool(false)),
            ]
        );
        // Keys stay byte-identical to the legacy spelling either way.
        assert_eq!(id.to_legacy_type_key().mangle(), "main__S_true_false");
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

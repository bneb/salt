//! WS-R S1: typed per-instance identity (S3-6 design D1, consolidated at
//! .round1-staging/s36design/CONSOLIDATED_DESIGN.md). Identity is the tuple
//! (template owner, ordered GenericArg values); const VALUES are first-class,
//! and the ONLY public constructor refuses value-spelled leaves so
//! param-name/value identities cannot masquerade as types. Unwired this
//! slice -- callers migrate in S2+ per C2_migration_slices.md.
//!
//! Param-spelled placeholder rejection is PENDING ParamId wiring (S2/S3):
//! today a placeholder spelled Struct("SIZE") is indistinguishable from a
//! user type named SIZE, so from_parts accepts it; the refusal narrows as
//! soon as owners carry ParamId registries.
#![allow(dead_code)] // Introduced ahead of caller migration (S2+).

use crate::types::{Type, TypeKey};
use crate::codegen::types::generic_arg::{GenericArg, is_value_spelled_leaf};

/// The mangled name of the template declaring the instantiation
/// ("main__Cache"). Same newtype family as ParamOwner.
pub(crate) type TemplateOwner = crate::codegen::types::generic_arg::ParamOwner;

/// Identity of one monomorphized fn instance: owner + method + ordered args.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct FnInstanceId {
    owner: TemplateOwner,
    member: String,
    args: Vec<GenericArg>,
}

/// Identity of one monomorphized struct instance:
/// `(template owner, ordered generic args)` with const VALUES first-class.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct InstanceId {
    owner: TemplateOwner,
    args: Vec<GenericArg>,
}

impl InstanceId {
    /// REFUSING constructor (the only public one): rejects value-spelled
    /// leaves ("64", "-7", "true") so const VALUES can never masquerade as
    /// type arguments. See module docs for the pending ParamId limitation.
    pub(crate) fn from_parts(owner: TemplateOwner, args: Vec<GenericArg>) -> Result<Self, String> {
        for arg in &args {
            if let GenericArg::Type(Type::Struct(n)) | GenericArg::Type(Type::Concrete(n, _)) = arg {
                if is_value_spelled_leaf(n) {
                    return Err(format!(
                        "instance argument `{n}` is a value spelling, not a type"
                    ));
                }
            }
        }
        Ok(Self { owner, args })
    }

    pub(crate) fn owner(&self) -> &TemplateOwner { &self.owner }

    pub(crate) fn args(&self) -> &[GenericArg] { &self.args }

    /// The TypeKey this instance corresponds to under TODAY'S string-keyed
    /// registry scheme. Golden pin: byte-equal to constructing the TypeKey
    /// directly from legacy spellings (see instance_id_tests).
    pub(crate) fn to_legacy_type_key(&self) -> TypeKey {
        let legacy: Vec<Type> = self.args.iter().map(|a| a.to_legacy_type()).collect();
        TypeKey {
            path: vec![],
            name: self.owner.as_str().to_string(),
            specialization: if legacy.is_empty() { None } else { Some(legacy) },
        }
    }
}

impl FnInstanceId {
    /// REFUSING constructor, validating through [`InstanceId::from_parts`]
    /// and carrying the validated args.
    pub(crate) fn from_parts(
        owner: TemplateOwner,
        member: String,
        args: Vec<GenericArg>,
    ) -> Result<Self, String> {
        let id = InstanceId::from_parts(owner, args)?;
        Ok(Self { owner: id.owner, member, args: id.args })
    }

    pub(crate) fn member(&self) -> &str { &self.member }
}

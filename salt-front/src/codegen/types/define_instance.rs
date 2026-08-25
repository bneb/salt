//! WS-R R2: single chokepoint through which struct/enum registry INSERTS
//! flow during specialization (`specialize_template` placeholder arms and
//! expansion-result overwrites today). Pure centralization so R3 can harden
//! ONE audit point; all policy guards (existence, Generic Guard, pending_set,
//! frozen, self-identity, protected name) stay with callers so early-return
//! semantics remain byte-identical to pre-R2 behavior.
//! Anchors: spec_template.rs:327-341 (placeholders), :353/:374 (overwrite).

use crate::registry::{EnumInfo, StructInfo};
use crate::types::{TypeKey};
use crate::codegen::context::{CodegenContext, LoweringContext};
use crate::codegen::types::generic_arg::ParamOwner;
use crate::codegen::types::instance_id::InstanceId;

/// Base mangled template name for a registry key (specialization stripped):
/// the InstanceId owner under the S2 validation contract.
fn base_mangled(key: &TypeKey) -> String {
    let mut parts: Vec<&str> = key.path.iter().map(|s| s.as_str()).collect();
    parts.push(&key.name);
    crate::common::mangling::Mangler::mangle(&parts)
}

/// S2 validation contract: every registered instance must construct a valid
/// typed identity. Total today -- refusals live at extraction (T-b/T-c) --
/// so this is a pure assertion point that S3 will upgrade to storage.
fn validate_instance(owner: ParamOwner, args: &[crate::types::Type]) -> InstanceId {
    InstanceId::from_legacy(owner, args)
}

impl<'a, 'ctx> LoweringContext<'a, 'ctx> {
    /// Registers one monomorphized struct instance under `key`.
    ///
    /// Semantics are EXACTLY `struct_registry.insert(key, info)`: any
    /// previous entry (e.g. the step-6 placeholder) is displaced, nothing
    /// is returned, no emission state is touched.
    pub(crate) fn define_struct_instance(&mut self, key: TypeKey, info: StructInfo) {
        let _typed = validate_instance(ParamOwner::mangled(&base_mangled(&key)), &info.specialization_args);
        self.struct_registry_mut().insert(key, info);
    }

    /// Registers one monomorphized enum instance under `key`.
    ///
    /// Semantics are EXACTLY `enum_registry.insert(key, info)`; same
    /// overwrite-in-place contract as [`Self::define_struct_instance`].
    pub(crate) fn define_enum_instance(&mut self, key: TypeKey, info: EnumInfo) {
        let _typed = validate_instance(ParamOwner::mangled(&base_mangled(&key)), &info.specialization_args);
        self.enum_registry_mut().insert(key, info);
    }
}

/// CodegenContext-receiver twins of the chokepoint above.
///
/// The accessor families differ (`RefMut` guard vs `&mut`, see
/// context/accessors.rs:150-161 vs :13-16), so the insert is expressed once
/// per receiver instead of forcing bootstrap callers through the
/// all-RefCell `with_lowering_ctx` lock (context.rs:1210). Semantics are
/// identical: overwrite-in-place `HashMap::insert`, nothing returned, no
/// emission state touched. Bootstrap note: these wrappers never read or
/// write `suppress_specialization`; `registry_init` keeps owning it.
impl<'a> CodegenContext<'a> {
    /// Registers one monomorphized struct instance under `key`.
    pub(crate) fn define_struct_instance(&self, key: TypeKey, info: StructInfo) {
        let _typed = validate_instance(ParamOwner::mangled(&base_mangled(&key)), &info.specialization_args);
        self.struct_registry_mut().insert(key, info);
    }

    /// Registers one monomorphized enum instance under `key`.
    pub(crate) fn define_enum_instance(&self, key: TypeKey, info: EnumInfo) {
        let _typed = validate_instance(ParamOwner::mangled(&base_mangled(&key)), &info.specialization_args);
        self.enum_registry_mut().insert(key, info);
    }
}

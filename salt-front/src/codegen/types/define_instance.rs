//! WS-R R2: single chokepoint through which struct/enum registry INSERTS
//! flow during specialization (`specialize_template` placeholder arms and
//! expansion-result overwrites today). Pure centralization so R3 can harden
//! ONE audit point; all policy guards (existence, Generic Guard, pending_set,
//! frozen, self-identity, protected name) stay with callers so early-return
//! semantics remain byte-identical to pre-R2 behavior.
//! Anchors: spec_template.rs:327-341 (placeholders), :353/:374 (overwrite).

use crate::registry::{EnumInfo, StructInfo};
use crate::types::TypeKey;
use crate::codegen::context::LoweringContext;

impl<'a, 'ctx> LoweringContext<'a, 'ctx> {
    /// Registers one monomorphized struct instance under `key`.
    ///
    /// Semantics are EXACTLY `struct_registry.insert(key, info)`: any
    /// previous entry (e.g. the step-6 placeholder) is displaced, nothing
    /// is returned, no emission state is touched.
    pub(crate) fn define_struct_instance(&mut self, key: TypeKey, info: StructInfo) {
        self.struct_registry_mut().insert(key, info);
    }

    /// Registers one monomorphized enum instance under `key`.
    ///
    /// Semantics are EXACTLY `enum_registry.insert(key, info)`; same
    /// overwrite-in-place contract as [`Self::define_struct_instance`].
    pub(crate) fn define_enum_instance(&mut self, key: TypeKey, info: EnumInfo) {
        self.enum_registry_mut().insert(key, info);
    }
}

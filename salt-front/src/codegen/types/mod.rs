//! Type system utilities for the Salt codegen module.
//!
//! This module provides:
//! - `canonical`: TypeID system for O(1) type identity comparison
//! - `provenance`: Pointer provenance tracking for GEP optimization

pub mod canonical;
pub mod provenance;
pub mod layout;
pub mod substitution;
pub mod numeric;
pub mod resolution;
pub mod mlir;
pub mod zero_attr;
pub mod emit;
pub mod traits;
pub mod specialization;
pub mod spec_template;
pub mod expansion;
pub mod generic_arg;
pub mod define_instance;
#[cfg(test)] mod numeric_tests;
#[cfg(test)] mod resolution_tests;
#[cfg(test)] mod generic_arg_tests;
#[cfg(test)] mod define_instance_tests;

pub use canonical::{TypeID, TypeIDRegistry};
pub use provenance::{ProvenanceMap, OriginMap, GlobalLVN};

//! HIR Types
//!
//! Re-exports the shared Type system from `crate::types` for now,
//! effectively bridging the AST types to HIR types.

pub use crate::types::{Type, TypeKey, Provenance};

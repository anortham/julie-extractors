//! Structural-fact pattern SPECS for language-local `builtins`.
//!
//! Specs live in sibling submodules; this file only declares them and
//! concatenates their slices in registry order.

use super::StructuralFactPatternSpec;

mod core;
mod extra;
mod jvm_native;
mod scripting;

pub(super) fn specs() -> Vec<StructuralFactPatternSpec> {
    [
        core::SPECS,
        jvm_native::SPECS,
        scripting::SPECS,
        extra::SPECS,
    ]
    .concat()
}

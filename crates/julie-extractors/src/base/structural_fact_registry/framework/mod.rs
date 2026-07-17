//! Structural-fact pattern SPECS for the `framework` registry family.
//!
//! Specs live in sibling submodules; this file only declares them and
//! concatenates their slices in registry order.

use super::StructuralFactPatternSpec;

mod aspnet_node;
mod frontend;
mod jvm_go_ruby;
mod kotlin_elixir;
mod php;
mod python;
mod rust;

pub(super) fn specs() -> Vec<StructuralFactPatternSpec> {
    [
        aspnet_node::SPECS,
        python::SPECS,
        jvm_go_ruby::SPECS,
        php::SPECS,
        kotlin_elixir::SPECS,
        rust::SPECS,
        frontend::SPECS,
    ]
    .concat()
}

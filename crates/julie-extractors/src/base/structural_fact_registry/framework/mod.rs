//! Structural-fact pattern SPECS for the `framework` registry family.
//!
//! Specs live in sibling submodules; this file only declares them and
//! concatenates their slices in registry order.

use super::StructuralFactPatternSpec;

mod aspnet_node;
mod dart;
mod frontend;
mod godot;
mod jvm_go;
mod kotlin_elixir;
mod lua;
mod php;
mod python;
mod r;
mod ruby;
mod rust;
mod swift;
mod zig;

#[cfg(test)]
pub(super) fn frontend_specs() -> &'static [StructuralFactPatternSpec] {
    frontend::SPECS
}

pub(super) fn specs() -> Vec<StructuralFactPatternSpec> {
    [
        aspnet_node::SPECS,
        python::SPECS,
        jvm_go::SPECS,
        ruby::SPECS,
        php::SPECS,
        kotlin_elixir::SPECS,
        rust::SPECS,
        lua::SPECS,
        r::SPECS,
        swift::SPECS,
        dart::SPECS,
        godot::SPECS,
        zig::SPECS,
        frontend::SPECS,
    ]
    .concat()
}

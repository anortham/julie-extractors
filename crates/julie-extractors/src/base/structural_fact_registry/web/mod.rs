//! Structural-fact pattern SPECS for the `web` registry family.
//!
//! Specs live in sibling submodules; this file only declares them and
//! concatenates their slices in registry order.

use super::StructuralFactPatternSpec;

mod css;
mod html;
mod nextjs;
mod nuxt;
mod react;
mod vue;

pub(super) fn specs() -> Vec<StructuralFactPatternSpec> {
    [
        css::SPECS,
        html::SPECS,
        vue::SPECS,
        react::SPECS,
        nextjs::SPECS,
        nuxt::SPECS,
    ]
    .concat()
}

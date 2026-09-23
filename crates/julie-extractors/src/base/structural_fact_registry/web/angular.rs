//! Angular router structural-fact pattern SPECS.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR, StructuralFactPatternSpec,
    key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[StructuralFactPatternSpec {
    pattern_id: "angular.route_definition.v1",
    languages: &["typescript"],
    query_family: "frontend_navigation",
    description: "An Angular Router route object in a `Routes` array or in the array passed to `RouterModule.forRoot/forChild` or `provideRouter`.",
    metadata_keys: &[
        K_PATTERN_VERSION,
        K_QUERY_FAMILY,
        K_FRAMEWORK,
        key(
            "library",
            STR,
            ALWAYS,
            "Routing library (\"angular_router\").",
        ),
        key(
            "source_kind",
            STR,
            ALWAYS,
            "Definition form (\"route_object\").",
        ),
        key(
            "route_path",
            STR,
            ALWAYS,
            "The route's own static `path` (may be empty).",
        ),
        key(
            "route_source",
            STR,
            ALWAYS,
            "Origin of the path (\"string_literal\").",
        ),
        key(
            "effective_route_template",
            STR,
            ALWAYS,
            "Full route template from the parent paths and the own path.",
        ),
        key(
            "parent_route_path",
            STR,
            OPT,
            "Effective template of the parent route when nested under `children`.",
        ),
        key(
            "route_component",
            STR,
            OPT,
            "The `component` identifier, when present.",
        ),
        key(
            "redirect_to",
            STR,
            OPT,
            "The static `redirectTo` target, when present.",
        ),
        key(
            "lazy_module_source",
            STR,
            OPT,
            "Module source imported by `loadChildren`, when present.",
        ),
        key(
            "lazy_component_source",
            STR,
            OPT,
            "Module source imported by `loadComponent`, when present.",
        ),
    ],
}];

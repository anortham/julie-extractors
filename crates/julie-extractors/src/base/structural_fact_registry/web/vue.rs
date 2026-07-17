//! Vue SFC and router structural-fact pattern SPECS.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, ARR, BOOL, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR,
    StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "vue.sfc_section.v1",
        languages: &["vue"],
        query_family: "component_structure",
        description: "A Vue single-file-component section (template/script/style).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "section_type",
                STR,
                ALWAYS,
                "SFC section kind (template/script/style).",
            ),
            key(
                "lang",
                STR,
                OPT,
                "The block's lang attribute (e.g. ts, scss), when present.",
            ),
            key(
                "setup",
                BOOL,
                ALWAYS,
                "True when the block carries the setup attribute.",
            ),
            key(
                "scoped",
                BOOL,
                ALWAYS,
                "True when the block carries the scoped attribute.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vue.template_directive.v1",
        languages: &["vue"],
        query_family: "component_template",
        description: "A Vue template directive (v-bind/v-on/v-if/…).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "directive",
                STR,
                ALWAYS,
                "Canonical directive name (v-bind, v-on, v-if, v-model, …).",
            ),
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "Raw attribute name as written (e.g. `:to`, `@click`).",
            ),
            key(
                "shorthand",
                BOOL,
                ALWAYS,
                "True when written in shorthand (`:`/`@`).",
            ),
            key(
                "argument",
                STR,
                OPT,
                "Directive argument (e.g. the event name for v-on), when present.",
            ),
            key(
                "modifiers",
                ARR,
                OPT,
                "Directive modifiers list; omitted when empty.",
            ),
            key(
                "expression",
                STR,
                OPT,
                "The attribute's bound expression/value, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vue.route_reference.v1",
        languages: &["vue"],
        query_family: "frontend_navigation",
        description: "A Vue Router navigation reference (router-link or navigation call).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin (router_link or router_navigation_expression).",
            ),
            key(
                "target_path",
                STR,
                ALWAYS,
                "Static route path being navigated to.",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "HTTP method for the navigation (always \"GET\").",
            ),
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "The source attribute name (e.g. `to`, `@click`).",
            ),
            key(
                "expression",
                STR,
                OPT,
                "Original bound expression when the path came from a binding.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vue.route_definition.v1",
        languages: &["vue", "javascript", "jsx", "tsx", "typescript"],
        query_family: "frontend_navigation",
        description: "A Vue Router route definition object.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "target_path",
                STR,
                ALWAYS,
                "The route's own static path string.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Definition origin (\"vue_router_route\").",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "Origin of the parsed path literal (\"string_literal\").",
            ),
            key(
                "parent_route_path",
                STR,
                OPT,
                "Parent route path when nested under another route object.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Parent+own path joined into the full route template.",
            ),
            key(
                "route_name",
                STR,
                OPT,
                "Named-route `name` property, when present.",
            ),
            key(
                "component_name",
                STR,
                OPT,
                "Identifier of the mapped component, when present.",
            ),
            key(
                "component_path",
                STR,
                OPT,
                "Import path resolved for the component identifier, when present.",
            ),
        ],
    },
];

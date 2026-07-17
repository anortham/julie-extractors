//! React router structural-fact pattern SPECS.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, BOOL, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR,
    StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "react.route_reference.v1",
        languages: &["javascript", "jsx", "tsx"],
        query_family: "frontend_navigation",
        description: "A React Router link reference (`<Link to>`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "library",
                STR,
                ALWAYS,
                "Routing library (\"react_router\").",
            ),
            key(
                "target_path",
                STR,
                ALWAYS,
                "Static route path from the `to` attribute.",
            ),
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "The source attribute name (\"to\").",
            ),
            key(
                "component_name",
                STR,
                ALWAYS,
                "The JSX component/tag name (e.g. Link).",
            ),
            key(
                "import_source",
                STR,
                ALWAYS,
                "Module the link component was imported from.",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "Origin of the parsed path literal (\"string_literal\").",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin (\"react_router_link\").",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "HTTP method for the navigation (always \"GET\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "react.route_definition.v1",
        languages: &["javascript", "jsx", "tsx", "typescript"],
        query_family: "frontend_navigation",
        description: "A React Router route definition (JSX <Route> or route object).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "library",
                STR,
                ALWAYS,
                "Routing library (\"react_router\").",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Definition form (jsx_route or route_object).",
            ),
            key(
                "route_path",
                STR,
                OPT,
                "The route's own static path (absent for index routes).",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "\"string_literal\" when a path exists, else \"index_route\".",
            ),
            key(
                "index_route",
                BOOL,
                OPT,
                "Present (true) only for index routes.",
            ),
            key(
                "route_component",
                STR,
                OPT,
                "Mapped component identifier, when present.",
            ),
            key(
                "route_id",
                STR,
                OPT,
                "Route object `id` property, when present.",
            ),
            key(
                "parent_route_path",
                STR,
                OPT,
                "Parent route path when nested.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Full joined route template from parent + own path.",
            ),
        ],
    },
];

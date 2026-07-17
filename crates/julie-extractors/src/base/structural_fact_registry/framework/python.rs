//! Python (FastAPI/Flask/Django) framework route SPECS.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, ARR, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR,
    StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "fastapi.route.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A static FastAPI path-operation decorator on a traced FastAPI/APIRouter receiver.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"decorator_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the decorator.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family normalized route template.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key("verb", STR, ALWAYS, "Uppercase HTTP method."),
            key("verb_source", STR, ALWAYS, "How the verb was attested."),
            key("router_prefix", STR, OPT, "Same-file APIRouter prefix."),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Router prefix joined with route template.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "fastapi.include_router.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A FastAPI include_router mount call.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of the included router argument.",
            ),
            key(
                "mount_path",
                STR,
                OPT,
                "Literal prefix mount path, when present.",
            ),
            key(
                "normalized_mount_path",
                STR,
                OPT,
                "Normalized mount path, when a literal prefix is present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "flask.route.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A static Flask route decorator on a traced Flask/Blueprint receiver.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"decorator_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the decorator.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family normalized route template.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key("verb", STR, ALWAYS, "Uppercase HTTP method."),
            key(
                "verb_source",
                STR,
                ALWAYS,
                "Default or attested verb source.",
            ),
            key(
                "blueprint",
                STR,
                OPT,
                "Blueprint name literal for blueprint-owned routes.",
            ),
            key("url_prefix", STR, OPT, "Same-file Blueprint url_prefix."),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Blueprint prefix joined with route template.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "flask.blueprint_registration.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A Flask register_blueprint mount call.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of the blueprint argument.",
            ),
            key(
                "mount_path",
                STR,
                OPT,
                "Literal url_prefix mount path, when present.",
            ),
            key(
                "normalized_mount_path",
                STR,
                OPT,
                "Normalized mount path, when a literal url_prefix is present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "django.url_pattern.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A Django path/re_path URL pattern with a static route argument.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"dsl_routing\")."),
            key("route_template", STR, ALWAYS, "Raw static route string."),
            key("route_syntax", STR, ALWAYS, "\"path\" or \"regex\"."),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Normalized route template for path syntax.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in path syntax.",
            ),
            key("route_name", STR, OPT, "Literal name= value."),
            key(
                "view_target",
                STR,
                ALWAYS,
                "Source text of the view argument.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "django.url_include.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A Django include mount inside a path() URL pattern.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("mount_path", STR, ALWAYS, "Raw path() prefix string."),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized mount path.",
            ),
            key(
                "included_module",
                STR,
                ALWAYS,
                "Included module literal or source text.",
            ),
            key("namespace", STR, OPT, "Literal namespace= value."),
        ],
    },
];

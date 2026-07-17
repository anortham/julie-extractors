//! Ktor and Phoenix framework route SPECS.
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
        pattern_id: "ktor.route.v1",
        languages: &["kotlin"],
        query_family: "framework",
        description: "A static Ktor verb call lexically contained in a routing{}/route{} lambda (restricted gate).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"call_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the verb call's first argument.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Enclosing route(\"/prefix\") scopes joined with the route template when a static prefix applies.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and Ktor {param} segments preserved as :param.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "Uppercase HTTP method attested from the bare verb identifier.",
            ),
            key(
                "verb_source",
                STR,
                ALWAYS,
                "How the verb was attested (\"attested\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "phoenix.route.v1",
        languages: &["elixir"],
        query_family: "framework",
        description: "A static Phoenix router verb-macro route joined to its same-file scope prefix.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"dsl_routing\")."),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the router verb macro.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and Phoenix :param segments preserved.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file enclosing scope prefix governing the route.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Scope prefix joined with the route template when a static prefix applies.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method (every Phoenix verb macro is verb-restricted).",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\").",
            ),
            key(
                "controller",
                STR,
                OPT,
                "Controller/plug module alias as written at the route.",
            ),
            key(
                "action",
                STR,
                OPT,
                "Controller action atom name (`:show` recorded as show).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "phoenix.resource_route.v1",
        languages: &["elixir"],
        query_family: "framework",
        description: "A Phoenix router `resources \"/x\", Ctrl` RESTful resource declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"resource_routing\").",
            ),
            key(
                "resource_path",
                STR,
                ALWAYS,
                "Raw static resource URI literal.",
            ),
            key(
                "normalized_resource_path",
                STR,
                ALWAYS,
                "Normalized resource path including same-file scope prefix.",
            ),
            key(
                "controller",
                STR,
                OPT,
                "Controller module alias when statically resolvable.",
            ),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file enclosing scope prefix governing the resource.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "phoenix.forward.v1",
        languages: &["elixir"],
        query_family: "framework",
        description: "A static Phoenix router `forward \"/lit\", Plug` prefix registration at its definition site.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_path",
                STR,
                ALWAYS,
                "Raw static forward path literal at this site.",
            ),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized forward path including enclosing same-file scope prefix.",
            ),
            key("mount_target", STR, ALWAYS, "Forwarded plug module alias."),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file enclosing scope prefix governing the forward.",
            ),
        ],
    },
];

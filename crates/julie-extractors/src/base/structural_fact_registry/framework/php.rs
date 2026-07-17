//! Laravel and Symfony framework route SPECS.
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
        pattern_id: "laravel.route.v1",
        languages: &["php"],
        query_family: "framework",
        description: "A static Laravel Route facade route joined to its same-file group prefix.",
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
                "Raw static route path from the Route facade call.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and Laravel {param}/{param?} segments preserved as :param.",
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
                "Same-file Route::prefix()/group(['prefix'=>...]) prefix governing the route.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Group prefix joined with the route template when a static prefix applies.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method; omitted for Route::any (accepts any method).",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\").",
            ),
            key(
                "controller_action",
                STR,
                OPT,
                "Controller action target (\"Ctrl@method\" or the literal action string) when statically resolvable.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "laravel.resource_route.v1",
        languages: &["php"],
        query_family: "framework",
        description: "A Laravel Route::resource/apiResource declaration.",
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
                "resource_name",
                STR,
                ALWAYS,
                "Raw static resource URI literal.",
            ),
            key(
                "resource_kind",
                STR,
                ALWAYS,
                "resource (7 RESTful actions) or api_resource (5, no create/edit).",
            ),
            key(
                "controller",
                STR,
                OPT,
                "Controller class name when a static Ctrl::class reference is given.",
            ),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file group prefix governing the resource.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "laravel.route_prefix.v1",
        languages: &["php"],
        query_family: "framework",
        description: "A static Laravel Route::prefix()/group(['prefix'=>...]) prefix at its definition site.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_path",
                STR,
                ALWAYS,
                "Raw static prefix literal at this site.",
            ),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized prefix path including enclosing same-file group scope.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "symfony.route.v1",
        languages: &["php"],
        query_family: "framework",
        description: "A static Symfony #[Route] attribute on a controller class or method.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"annotation_routing\").",
            ),
            key(
                "attribute_kind",
                STR,
                ALWAYS,
                "class_route/http_method/request_mapping shape.",
            ),
            key("route_template", STR, ALWAYS, "Raw static route template."),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and Symfony {param} segments preserved as :param.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "class_route_template",
                STR,
                OPT,
                "Nearest class-level #[Route] template.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Class and method templates joined.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method when methods= restricts the route; omitted when any method is accepted.",
            ),
            key("verb_source", STR, OPT, "How the verb was attested."),
        ],
    },
];

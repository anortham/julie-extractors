//! go_router and shelf_router SPECS.
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
        pattern_id: "go_router.route_definition.v1",
        languages: &["dart"],
        query_family: "frontend_navigation",
        description: "A go_router `GoRoute(path:)` definition with a static path, joined to the paths of the same-file `GoRoute` routes that nest it, in a file that imports go_router.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("route_path", STR, ALWAYS, "The route's own static `path:`."),
            key(
                "parent_route_path",
                STR,
                OPT,
                "The effective path of the enclosing `GoRoute`, when nested.",
            ),
            key(
                "effective_route_template",
                STR,
                ALWAYS,
                "The route path joined with its same-file parent paths.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and `:param` segments preserved.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key("route_name", STR, OPT, "Static `name:` of the route."),
            key(
                "route_component",
                STR,
                OPT,
                "Widget class the `builder:` or `pageBuilder:` closure constructs.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "go_router.route_reference.v1",
        languages: &["dart"],
        query_family: "frontend_navigation",
        description: "A go_router navigation call (`go`, `push`, `replace`, `pushReplacement`, or their `Named` forms) with a static location or route name, in a file that imports go_router.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "navigation_method",
                STR,
                ALWAYS,
                "The navigation method called (for example \"go\" or \"pushNamed\").",
            ),
            key(
                "target_path",
                STR,
                OPT,
                "Static location of a path navigation.",
            ),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Normalized location of a path navigation.",
            ),
            key(
                "route_name",
                STR,
                OPT,
                "Static route name of a `Named` navigation.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "shelf_router.route.v1",
        languages: &["dart"],
        query_family: "framework",
        description: "A shelf_router route with a static path: a `@Route.<verb>(path)` or `@Route(verb, path)` handler annotation, or a verb call on a same-file `Router()`, in a file that imports shelf_router.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"annotation\" or \"router_call\").",
            ),
            key("route_template", STR, ALWAYS, "Raw static route path."),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and shelf `<param>` segments as `:param`.",
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
                "How the verb was attested (\"attested\").",
            ),
            key(
                "handler",
                STR,
                OPT,
                "Annotated method name, or the handler argument when it is a name.",
            ),
        ],
    },
];

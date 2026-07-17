//! Axum and Actix framework route SPECS.
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
        pattern_id: "axum.route.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static axum `Router::new().route(\"/x\", get(h))` route, one fact per method-router verb.",
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
                "Raw static route path passed to `.route`.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key; axum 0.8 `{id}` brace captures normalize to `:id` segments.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template (axum 0.8 brace captures; a 0.7 `:id` template is an honest under-report and yields none).",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method for a verb-restricted method router; omitted for `any`/`any_service`.",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\"); omitted with the verb for `any`/`any_service`.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "axum.nest.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static axum `Router::new().nest(\"/lit\", sub_router)` prefix registration at its definition site.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_path",
                STR,
                ALWAYS,
                "Raw static nest path literal at this site.",
            ),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized nest path (axum 0.8 brace captures preserved as `:param`).",
            ),
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of the nested sub-router expression (a cross-file target; no route join is guessed).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "actix.attribute_route.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static actix-web attribute-macro route (`#[get(\"/x\")]` / `#[route(\"/x\", method = \"GET\")]`) on a handler fn, one fact per verb.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"attribute\")."),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the attribute macro's first argument.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key; actix `{id}` brace captures normalize to `:id` segments.",
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
                "Uppercase HTTP method from the macro name (`#[get]`→GET) or a `method = \"VERB\"` argument (`#[route]`).",
            ),
            key(
                "verb_source",
                STR,
                ALWAYS,
                "How the verb was attested (\"attested\"; attribute-macro verbs are always explicit).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "actix.scope_route.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static actix-web scope-chained route (`web::scope(\"/api\").route(\"/x\", web::post().to(h))`) with a same-file scope prefix.",
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
                "Raw static route path passed to `.route` (without the scope prefix).",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key computed from the effective (scope prefix + route) template; actix `{id}` brace captures normalize to `:id`.",
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
                OPT,
                "Uppercase HTTP method from the `web::<verb>()` method router; omitted for the method-agnostic `web::route()`.",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\"); omitted with the verb for `web::route()`.",
            ),
            key(
                "route_group_prefix",
                STR,
                ALWAYS,
                "Same-file `web::scope(\"/lit\")` prefix the route chains off (scope routes are always scoped).",
            ),
            key(
                "effective_route_template",
                STR,
                ALWAYS,
                "Scope prefix joined with the route template.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "actix.mount.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static actix-web `web::scope(\"/lit\").configure(fn)` / `.service(sub)` mount, the scope prefix recorded at its registration site.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_path",
                STR,
                ALWAYS,
                "Raw static scope path literal at this site.",
            ),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized scope path (actix brace captures preserved as `:param`).",
            ),
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of the `configure`/`service` target (a cross-file target; no route join is guessed).",
            ),
        ],
    },
];

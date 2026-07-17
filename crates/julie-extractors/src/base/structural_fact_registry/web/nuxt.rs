//! Nuxt route structural-fact pattern SPECS.
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
        pattern_id: "nuxt.route_reference.v1",
        languages: &["vue"],
        query_family: "frontend_navigation",
        description: "A Nuxt `<NuxtLink to>` navigation reference.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "target_path",
                STR,
                ALWAYS,
                "Static route path from the `to` attribute.",
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
                "The source attribute name (\"to\").",
            ),
            key("component_name", STR, ALWAYS, "The NuxtLink tag name."),
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
                "Reference origin (\"nuxt_link\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "nuxt.file_route.v1",
        languages: &["vue", "javascript", "jsx", "tsx", "typescript"],
        query_family: "frontend_navigation",
        description: "A Nuxt file-based page route.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("router", STR, ALWAYS, "Router family (\"pages\")."),
            key(
                "file_convention",
                STR,
                ALWAYS,
                "File convention (\"page\").",
            ),
            key(
                "route_path",
                STR,
                ALWAYS,
                "Route path derived from the file path.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Route origin (\"nuxt_file_route\").",
            ),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Dynamic-normalized route template, when dynamic.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Dynamic segment names; omitted when empty.",
            ),
            key(
                "route_group_segments",
                ARR,
                OPT,
                "Group segment names; omitted when empty.",
            ),
            key(
                "parallel_route_segments",
                ARR,
                OPT,
                "Parallel-route slot names; omitted when empty.",
            ),
            key(
                "intercepting_route_markers",
                ARR,
                OPT,
                "Intercepting-route markers; omitted when empty.",
            ),
            key(
                "intercepted_route_segments",
                ARR,
                OPT,
                "Intercepted segment names; omitted when empty.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "nuxt.server_route.v1",
        languages: &["javascript", "typescript"],
        query_family: "framework",
        description: "A Nuxt server route (server/api handler).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("router", STR, ALWAYS, "Router family (\"server\")."),
            key(
                "route_path",
                STR,
                ALWAYS,
                "Server route path derived from the file path.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Route origin (\"nuxt_server_route\").",
            ),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Dynamic-normalized route template, when dynamic.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Dynamic segment names; omitted when empty.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "HTTP method from a filename suffix (.get/.post/…), when present.",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\"); inserted only alongside verb.",
            ),
        ],
    },
];

//! Next.js route structural-fact pattern SPECS.
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
        pattern_id: "nextjs.route_reference.v1",
        languages: &["javascript", "jsx", "tsx"],
        query_family: "frontend_navigation",
        description: "A Next.js `<Link href>` navigation reference.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "target_path",
                STR,
                ALWAYS,
                "Static route path from the href.",
            ),
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "The source attribute name (\"href\").",
            ),
            key(
                "component_name",
                STR,
                ALWAYS,
                "The Link component/tag name.",
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
                "Origin of the parsed path (\"string_literal\" or \"object_pathname_literal\").",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin (\"next_link\").",
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
        pattern_id: "nextjs.file_route.v1",
        languages: &["javascript", "jsx", "tsx", "typescript"],
        query_family: "frontend_navigation",
        description: "A Next.js file-based page route.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "router",
                STR,
                ALWAYS,
                "Router family (\"app\" or \"pages\").",
            ),
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
                "Route origin (\"nextjs_file_route\").",
            ),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Path with dynamic segments normalized, when dynamic.",
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
                "App Router `(group)` segment names; omitted when empty.",
            ),
            key(
                "parallel_route_segments",
                ARR,
                OPT,
                "App Router `@slot` parallel-route names; omitted when empty.",
            ),
            key(
                "intercepting_route_markers",
                ARR,
                OPT,
                "Intercepting-route markers ((.)/(..)/…); omitted when empty.",
            ),
            key(
                "intercepted_route_segments",
                ARR,
                OPT,
                "Segments targeted by intercepting routes; omitted when empty.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "nextjs.route_handler.v1",
        languages: &["javascript", "typescript"],
        query_family: "framework",
        description: "A Next.js App Router route handler export (one fact per HTTP verb).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("router", STR, ALWAYS, "Router family (App Router \"app\")."),
            key(
                "file_convention",
                STR,
                ALWAYS,
                "File convention (\"route\").",
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
                "Route origin (\"nextjs_route_handler\").",
            ),
            key("verb", STR, ALWAYS, "Exported HTTP verb (GET/POST/…)."),
            key(
                "verb_source",
                STR,
                ALWAYS,
                "How the verb was attested (\"attested\").",
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
                "App Router group segment names; omitted when empty.",
            ),
            key(
                "parallel_route_segments",
                ARR,
                OPT,
                "App Router parallel-route names; omitted when empty.",
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
];

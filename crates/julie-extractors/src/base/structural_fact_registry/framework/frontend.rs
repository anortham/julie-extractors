//! Frontend-interaction and .NET markup SPECS (htmx, Alpine, Blazor, Razor).
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, ARR, BOOL, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OBJARR, OPT, STR,
    StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "htmx.attribute.v1",
        languages: &["html", "razor", "javascript", "jsx", "tsx", "vue"],
        query_family: "frontend_interaction",
        description: "An htmx attribute (hx-* or data-hx-*).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "Canonical htmx attribute name (normalized to hx-* form).",
            ),
            key(
                "data_prefix",
                BOOL,
                OPT,
                "Present and true only when the data-hx-* form was used.",
            ),
            key(
                "attribute_value",
                STR,
                OPT,
                "Raw attribute value, when the attribute has a value.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "HTTP method for request attributes (hx-get/post/…); absent otherwise.",
            ),
            key(
                "target_path",
                STR,
                OPT,
                "Static request path from the attribute value, when applicable.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "alpine.directive.v1",
        languages: &["html", "razor"],
        query_family: "frontend_interaction",
        description: "An Alpine.js directive (x-*, @, or :).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "directive",
                STR,
                ALWAYS,
                "Canonical Alpine directive name (e.g. \"x-on\", \"x-bind\").",
            ),
            key(
                "argument",
                STR,
                OPT,
                "Directive argument after the colon (e.g. event name), when present.",
            ),
            key(
                "modifiers",
                ARR,
                OPT,
                "Dot-modifiers (e.g. [\"prevent\", \"stop\"]); omitted when empty.",
            ),
            key(
                "expression",
                STR,
                OPT,
                "The directive's value/expression, when present.",
            ),
            key(
                "shorthand",
                BOOL,
                ALWAYS,
                "True when the shorthand form (@… or :…) was used.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "blazor.component_reference.v1",
        languages: &["razor"],
        query_family: "component_reference",
        description: "A PascalCase Blazor component tag reference in a Razor component.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("tag", STR, ALWAYS, "Referenced PascalCase component tag."),
            key(
                "containing_component",
                STR,
                ALWAYS,
                "Razor component filename stem containing the reference.",
            ),
            key(
                "namespace_context",
                ARR,
                ALWAYS,
                "Locally declared @namespace and @using values, in source order.",
            ),
            key(
                "generic_arguments",
                OBJARR,
                ALWAYS,
                "Static T+Uppercase attribute candidate evidence as name/value objects, in source order; naming-convention syntax only, not resolved generic semantics.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.page_directive.v1",
        languages: &["razor"],
        query_family: "component_routing",
        description: "A Razor `@page` directive.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("directive", STR, ALWAYS, "Directive kind (\"page\")."),
            key(
                "route",
                STR,
                ALWAYS,
                "Route string from the @page directive.",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Same route value under the template key, for aspnet consistency.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Route template normalized for HTTP-boundary joins.",
            ),
            key(
                "route_parameter_count",
                NUM,
                ALWAYS,
                "Count of {param} segments parsed from the route.",
            ),
            key(
                "has_route_constraints",
                BOOL,
                ALWAYS,
                "True when any route parameter carries a :constraint.",
            ),
            key(
                "route_parameters",
                OBJARR,
                ALWAYS,
                "Parsed route parameters as a JSON array of objects (empty when the \
                 route has none). Each object carries `name` (String), `optional` \
                 (Bool), and `catch_all` (Bool) always, plus `constraint` (String) \
                 only when the {param:constraint} form is used.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.code_block.v1",
        languages: &["razor"],
        query_family: "component_code",
        description: "A Razor `@code`/`@functions` block.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "block_type",
                STR,
                ALWAYS,
                "Razor block type (\"code\" or \"functions\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.route_reference.v1",
        languages: &["csharp", "razor"],
        query_family: "frontend_navigation",
        description: "A Blazor NavigationManager call or static Razor href route reference.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "target_path",
                STR,
                ALWAYS,
                "Raw static route path from the navigation target.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin (navigate_to, navigate_to_login, or href).",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "Origin of the parsed route (string_literal).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.template_expression.v1",
        languages: &["razor"],
        query_family: "component_template",
        description: "A Razor template expression (`@expr` or `@(expr)`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "expression",
                STR,
                ALWAYS,
                "The Razor expression text with the leading @ stripped.",
            ),
            key(
                "implicit",
                BOOL,
                ALWAYS,
                "True for implicit expressions (vs explicit @(...)).",
            ),
        ],
    },
];

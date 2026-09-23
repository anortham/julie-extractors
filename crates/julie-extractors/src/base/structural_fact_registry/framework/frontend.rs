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
                "normalized_route_template",
                STR,
                OPT,
                "Parser-proven literal path segments with dynamic placeholders.",
            ),
            key(
                "route_template_uncertainty",
                STR,
                OPT,
                "Partial or unknown interpolation evidence; never exact route equivalence.",
            ),
            key(
                "route_template_segments",
                OBJARR,
                OPT,
                "Ordered literal and dynamic source segments.",
            ),
            key(
                "declaration_start_byte",
                NUM,
                OPT,
                "Start byte of a consumed dictionary entry declaration.",
            ),
            key(
                "declaration_end_byte",
                NUM,
                OPT,
                "End byte of a consumed dictionary entry declaration.",
            ),
            key(
                "binding_source",
                STR,
                OPT,
                "Consumed object when a dictionary/object is bound to markup.",
            ),
            key(
                "value_source",
                STR,
                OPT,
                "String literal or dynamic expression for consumed object entries.",
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
        description: "A Blazor NavigationManager call or Razor `<a>`/`<NavLink>` href route reference.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "target_path",
                STR,
                ALWAYS,
                "Route path the target names. A base-relative target is \
                 resolved against the `/` app base path. Razor expressions \
                 and interpolation holes stay as `{expression}` segments.",
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
                "Origin of the parsed route: string_literal, interpolated_string \
                 (C# `$\"...\"`), or template_expression (href with `@expr`).",
            ),
            key(
                "base_relative",
                BOOL,
                OPT,
                "True when the written target has no leading `/` and resolves \
                 against the app base path.",
            ),
            key(
                "raw_target",
                STR,
                OPT,
                "The target as written, present when base_relative is true.",
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
    StructuralFactPatternSpec {
        pattern_id: "razor.mvc_link.v1",
        languages: &["razor"],
        query_family: "server_navigation",
        description: "An ASP.NET Core MVC or Razor Pages link to a page or controller action: \
                      an `asp-page`/`asp-controller`/`asp-action` tag helper, \
                      `Html.ActionLink`, `Html.BeginForm`, `Url.Action`, or `Url.Page`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "target_kind",
                STR,
                ALWAYS,
                "`page` for a Razor Pages target, `action` for a controller action.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Link origin: tag_helper, html_helper, or url_helper.",
            ),
            key(
                "page",
                STR,
                OPT,
                "Static page path (`asp-page`, `Url.Page`).",
            ),
            key(
                "page_handler",
                STR,
                OPT,
                "Static page handler name (`asp-page-handler`).",
            ),
            key("controller", STR, OPT, "Static controller name."),
            key("action", STR, OPT, "Static action name."),
            key("area", STR, OPT, "Static area name (`asp-area`)."),
            key(
                "tag",
                STR,
                OPT,
                "Element tag name, present for tag-helper links.",
            ),
            key(
                "route_value_names",
                ARR,
                OPT,
                "Route value names from `asp-route-*` attributes, in source order; \
                 present for tag-helper links.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.partial_reference.v1",
        languages: &["razor"],
        query_family: "view_composition",
        description: "A partial view reference: `<partial name>` or an `Html.Partial`, \
                      `PartialAsync`, `RenderPartial`, or `RenderPartialAsync` call with a \
                      static name.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "partial_name",
                STR,
                ALWAYS,
                "Partial view name or path as written.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin: tag_helper or html_helper.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.view_component_reference.v1",
        languages: &["razor"],
        query_family: "view_composition",
        description: "A view component reference: `Component.InvokeAsync(\"Name\")`, \
                      `Component.InvokeAsync<T>()`, or a `<vc:name>` tag helper.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "component_name",
                STR,
                ALWAYS,
                "View component name. A `<vc:shopping-cart>` tag gives the \
                 PascalCase form (`ShoppingCart`); other forms keep the name as written.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin: tag_helper or component_invoke.",
            ),
            key(
                "tag",
                STR,
                OPT,
                "The `vc:` tag as written, for tag-helper references.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.layout_reference.v1",
        languages: &["razor"],
        query_family: "view_composition",
        description: "A layout reference: a `Layout = \"_Layout\"` assignment or a Blazor \
                      `@layout` directive.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("layout", STR, ALWAYS, "Layout name or type as written."),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin: assignment or directive.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.model_binding.v1",
        languages: &["razor"],
        query_family: "form_binding",
        description: "A tag-helper model binding: an `asp-for` or `asp-validation-for` \
                      attribute.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "model_expression",
                STR,
                ALWAYS,
                "Bound model expression as written, without a leading `@`.",
            ),
            key(
                "attribute",
                STR,
                ALWAYS,
                "Binding attribute: asp-for or asp-validation-for.",
            ),
            key("tag", STR, ALWAYS, "Element tag name."),
        ],
    },
];

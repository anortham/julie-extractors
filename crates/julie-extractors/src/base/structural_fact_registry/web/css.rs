//! CSS structural-fact pattern SPECS.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, ARR, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "css.selector_rule.v1",
        languages: &["css", "vue", "html"],
        query_family: "stylesheet_structure",
        description: "A CSS selector rule set.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "selector",
                STR,
                ALWAYS,
                "Raw CSS selector text of the rule set.",
            ),
            key(
                "selector_kind",
                STR,
                ALWAYS,
                "Coarse selector classification (class/id/pseudo/type/selector_list/compound).",
            ),
            key(
                "declaration_count",
                NUM,
                ALWAYS,
                "Count of declarations directly inside the rule block; nested rules are not counted.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.custom_property.v1",
        languages: &["css", "vue", "html"],
        query_family: "stylesheet_structure",
        description: "A CSS custom property declaration (`--name`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "property_name",
                STR,
                ALWAYS,
                "The `--*` CSS custom property name.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.media_query.v1",
        languages: &["css", "vue", "html"],
        query_family: "responsive_design",
        description: "A CSS `@media` query.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "query",
                STR,
                OPT,
                "The `@media` prelude/condition text, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.keyframes.v1",
        languages: &["css", "vue", "html"],
        query_family: "animation",
        description: "A CSS `@keyframes` animation.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "animation_name",
                STR,
                OPT,
                "The `@keyframes` animation name, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.supports.v1",
        languages: &["css", "vue", "html"],
        query_family: "feature_query",
        description: "A CSS `@supports` feature query.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "condition",
                STR,
                OPT,
                "The `@supports` prelude/condition text, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.container.v1",
        languages: &["css", "vue", "html"],
        query_family: "responsive_design",
        description: "A CSS `@container` query.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "condition",
                STR,
                OPT,
                "The `@container` prelude/condition text, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.font_face.v1",
        languages: &["css", "vue", "html"],
        query_family: "stylesheet_structure",
        description: "A CSS `@font-face` rule.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "at_rule",
                STR,
                ALWAYS,
                "The at-rule keyword (\"@font-face\").",
            ),
            key(
                "font_family",
                STR,
                OPT,
                "The unquoted `font-family` descriptor, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.layer.v1",
        languages: &["css", "vue", "html"],
        query_family: "stylesheet_structure",
        description: "A CSS `@layer` rule.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "layer_name",
                STR,
                OPT,
                "The `@layer` name prelude, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.charset.v1",
        languages: &["css", "vue", "html"],
        query_family: "stylesheet_structure",
        description: "A CSS `@charset` rule.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "encoding",
                STR,
                ALWAYS,
                "Declared charset encoding text (including quotes when present in source).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.namespace.v1",
        languages: &["css", "vue", "html"],
        query_family: "stylesheet_structure",
        description: "A CSS `@namespace` rule.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "namespace",
                STR,
                ALWAYS,
                "The `@namespace` prelude text (prefix and/or URL).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.import.v1",
        languages: &["css", "vue", "html"],
        query_family: "stylesheet_structure",
        description: "A CSS `@import` of another stylesheet.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "url",
                STR,
                ALWAYS,
                "The unquoted import target path or URL.",
            ),
            key(
                "media",
                STR,
                OPT,
                "Media, supports, or layer conditions after the target, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.scope.v1",
        languages: &["css", "vue", "html"],
        query_family: "stylesheet_structure",
        description: "A CSS `@scope` rule.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("root", STR, OPT, "The scoping root selector, when present."),
            key(
                "limit",
                STR,
                OPT,
                "The scoping limit selector after `to`, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.tailwind_apply.v1",
        languages: &["css", "vue", "html"],
        query_family: "directives",
        description: "A Tailwind CSS `@apply` of utility or component classes.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "classes",
                ARR,
                ALWAYS,
                "The applied class names in source order.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.tailwind_directive.v1",
        languages: &["css", "vue", "html"],
        query_family: "directives",
        description: "A Tailwind CSS at-rule directive such as `@tailwind`, `@utility`, or `@theme`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "directive",
                STR,
                ALWAYS,
                "The directive keyword without `@` (`tailwind`, `utility`, `theme`).",
            ),
            key(
                "argument",
                STR,
                OPT,
                "The directive prelude (`base`, a utility name), when present.",
            ),
        ],
    },
];

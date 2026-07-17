//! CSS structural-fact pattern SPECS.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec, key,
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
                "Coarse selector classification (class/id/pseudo/selector_list/compound).",
            ),
            key(
                "declaration_count",
                NUM,
                ALWAYS,
                "Count of declarations inside the rule block.",
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
];

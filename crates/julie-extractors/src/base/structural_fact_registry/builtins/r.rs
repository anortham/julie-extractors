//! Built-in language-local SPECS for R.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, ARR, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "r.library_call.v1",
        languages: &["r"],
        query_family: "imports",
        description: "An R package load: `library()`, `require()`, `requireNamespace()`, `pacman::p_load()`, `box::use()`, or `import::from()`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "load_kind",
                STR,
                ALWAYS,
                "Which load form was used (`library`, `require`, `requireNamespace`, `pacman::p_load`, `box::use`, or `import::from`).",
            ),
            key(
                "package_name",
                STR,
                OPT,
                "The package name argument (quotes stripped).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "r.pipe_expression.v1",
        languages: &["r"],
        query_family: "pipeline",
        description: "An R pipe chain (`|>`, `%>%`, `%<>%`, `%T>%`, or `%$%`): one fact for the outermost pipe of each chain.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "pipe_operator",
                STR,
                ALWAYS,
                "The operator of the outermost pipe.",
            ),
            key(
                "stage_count",
                NUM,
                ALWAYS,
                "The number of pipe operators in the chain.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "r.namespace_directive.v1",
        languages: &["r"],
        query_family: "module",
        description: "A directive in an R package `NAMESPACE` file: `export`, `exportPattern`, `exportClasses`, `exportClassPattern`, `exportMethods`, `S3method`, `import`, `importFrom`, `importClassesFrom`, `importMethodsFrom`, or `useDynLib`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("directive", STR, ALWAYS, "The directive name."),
            key(
                "arguments",
                ARR,
                ALWAYS,
                "The directive's arguments, unquoted, in source order.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "r.formula_expression.v1",
        languages: &["r"],
        query_family: "modeling",
        description: "An R model formula: a two-sided `y ~ x` or one-sided `~ x` expression whose operator is `~`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "formula_text",
                STR,
                ALWAYS,
                "The full text of the R model formula.",
            ),
        ],
    },
];

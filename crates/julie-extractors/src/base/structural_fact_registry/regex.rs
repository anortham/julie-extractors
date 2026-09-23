//! Structural-fact pattern SPECS for the `regex` registry family.
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries emitted by
//! the regex arm of `base/data_structural_facts.rs`. Public registry access remains through
//! [`super::structural_fact_pattern_specs`].

use super::{
    ALWAYS, BOOL, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    // Regex
    StructuralFactPatternSpec {
        pattern_id: "regex.capture_group.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex anonymous capturing group.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "capture_index",
                NUM,
                ALWAYS,
                "1-based ordinal index of this capturing group.",
            ),
            key(
                "named",
                BOOL,
                ALWAYS,
                "Always false, distinguishing anonymous groups from named ones.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.named_capture.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex named capturing group.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "capture_name",
                STR,
                ALWAYS,
                "The declared name of the named capture group.",
            ),
            key(
                "capture_index",
                NUM,
                ALWAYS,
                "1-based ordinal index of this capturing group.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.lookaround.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex lookahead or lookbehind assertion.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("direction", STR, ALWAYS, "\"lookahead\" or \"lookbehind\"."),
            key("polarity", STR, ALWAYS, "\"positive\" or \"negative\"."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.character_class.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex character class (`[...]`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "negated",
                BOOL,
                ALWAYS,
                "Whether the class is negated (starts with `[^`).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.quantifier.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex quantifier.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "quantifier",
                STR,
                ALWAYS,
                "Trimmed raw text of the quantifier (e.g. \"*\", \"+\", \"{2,4}\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.alternation.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex alternation (`a|b`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "branch_count",
                NUM,
                ALWAYS,
                "Number of alternation branches.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.anchor.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex anchor assertion.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "anchor_kind",
                STR,
                ALWAYS,
                "Classified anchor kind (start/end/word_boundary/…).",
            ),
        ],
    },
];

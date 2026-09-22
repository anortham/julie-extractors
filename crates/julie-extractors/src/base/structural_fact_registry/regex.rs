//! Structural-fact pattern SPECS for the `regex` registry family.

use super::{
    ALWAYS, BOOL, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
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
                "Trimmed raw text of the quantifier (e.g. \"*\", \"+\", \"{2,4}\", \"++\").",
            ),
            key(
                "possessive",
                BOOL,
                OPT,
                "True for a possessive quantifier (`a++`); absent otherwise.",
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
    StructuralFactPatternSpec {
        pattern_id: "regex.inline_flags.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex inline flag group (`(?i)` or `(?i-s:...)`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "enabled_flags",
                STR,
                ALWAYS,
                "Flag letters the group turns on, in source order (may be empty).",
            ),
            key(
                "disabled_flags",
                STR,
                ALWAYS,
                "Flag letters after `-` that the group turns off (may be empty).",
            ),
            key(
                "scoped",
                BOOL,
                ALWAYS,
                "True when the flags apply only to the group's own sub-pattern.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.backreference.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex backreference (`\\1`, `\\k<name>`, `(?P=name)`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "form",
                STR,
                ALWAYS,
                "\"numeric\", \"named\" (`\\k<name>`) or \"python_named\" (`(?P=name)`).",
            ),
            key(
                "capture_index",
                NUM,
                OPT,
                "Target capture index of a numeric backreference.",
            ),
            key(
                "capture_name",
                STR,
                OPT,
                "Target group name of a named backreference.",
            ),
            key(
                "resolved",
                BOOL,
                ALWAYS,
                "Whether the same pattern declares the target group.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.quoted_literal.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex quoted literal span (`\\Q...\\E`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "literal_text",
                STR,
                ALWAYS,
                "Text between `\\Q` and `\\E`, matched literally.",
            ),
            key(
                "closed",
                BOOL,
                ALWAYS,
                "False when no `\\E` closes the span before the end of its term.",
            ),
        ],
    },
];

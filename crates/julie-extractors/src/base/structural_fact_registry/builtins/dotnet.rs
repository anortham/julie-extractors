//! Built-in language-local SPECS for VB.NET.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    BASE_KEYS, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "vbnet.handles_clause.v1",
        languages: &["vbnet"],
        query_family: "events",
        description: "A VB.NET `Handles` clause on a method.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "handles_target",
                STR,
                OPT,
                "The event target named after the `Handles` keyword.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vbnet.implements_clause.v1",
        languages: &["vbnet"],
        query_family: "interface",
        description: "A VB.NET `Implements` clause on a member.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "implements_target",
                STR,
                OPT,
                "The interface member named after the `Implements` keyword.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vbnet.event_declaration.v1",
        languages: &["vbnet"],
        query_family: "events",
        description: "A VB.NET `Event` declaration.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "vbnet.attribute.v1",
        languages: &["vbnet"],
        query_family: "metadata",
        description: "A VB.NET attribute use.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "attribute_name",
                STR,
                OPT,
                "Last segment of the attribute name.",
            ),
            key(
                "qualified_name",
                STR,
                OPT,
                "Dotted name as written, when qualified.",
            ),
            key(
                "target",
                STR,
                OPT,
                "Written target: `Assembly` or `Module`.",
            ),
        ],
    },
];

//! Structural-fact pattern SPECS for PostgreSQL DDL objects in the `sql`
//! registry family: policies, extensions, sequences, types, and roles.
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries. Public
//! registry access remains through [`super::structural_fact_pattern_specs`].

use super::{
    ALWAYS, ARR, BOOL, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec,
    key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "sql.policy_definition.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A PostgreSQL `CREATE POLICY` row-security policy.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("policy_name", STR, ALWAYS, "Name of the policy."),
            key("table_name", STR, OPT, "Table the policy applies to."),
            key("schema_name", STR, OPT, "Schema qualifier of the table."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.extension.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A PostgreSQL `CREATE EXTENSION` statement.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("extension_name", STR, ALWAYS, "Name of the extension."),
            key(
                "if_not_exists",
                BOOL,
                ALWAYS,
                "Whether the statement has IF NOT EXISTS.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.sequence_definition.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL `CREATE SEQUENCE` definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("sequence_name", STR, ALWAYS, "Name of the sequence."),
            key("schema_name", STR, OPT, "Schema qualifier of the sequence."),
            key("start", NUM, OPT, "START value when it is an integer."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.type_definition.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL `CREATE TYPE` definition: an enum, a composite, or another type.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("type_name", STR, ALWAYS, "Name of the type."),
            key("schema_name", STR, OPT, "Schema qualifier of the type."),
            key(
                "type_kind",
                STR,
                ALWAYS,
                "Type shape: \"enum\", \"composite\", or \"other\".",
            ),
            key(
                "enum_values",
                ARR,
                OPT,
                "Enum labels in declaration order (enum types only).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.role_definition.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL `CREATE ROLE` statement.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("role_name", STR, ALWAYS, "Name of the role."),
        ],
    },
];

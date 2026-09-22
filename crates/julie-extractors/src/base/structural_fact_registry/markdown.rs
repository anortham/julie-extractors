//! Structural-fact pattern SPECS for the `markdown` registry family.
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries emitted by
//! the markdown arm of `base/data_structural_facts.rs`. Public registry access remains through
//! [`super::structural_fact_pattern_specs`].

use super::{
    ALWAYS, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    // Markdown
    StructuralFactPatternSpec {
        pattern_id: "markdown.frontmatter.v1",
        languages: &["markdown"],
        query_family: "document_metadata",
        description: "A Markdown frontmatter block (YAML `---` or TOML `+++`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "format",
                STR,
                ALWAYS,
                "Frontmatter serialization format (\"toml\" or \"yaml\").",
            ),
            key(
                "key_count",
                NUM,
                ALWAYS,
                "Count of non-empty, non-comment frontmatter key lines.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.heading.v1",
        languages: &["markdown"],
        query_family: "document_structure",
        description: "A Markdown ATX heading.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("level", NUM, ALWAYS, "Heading depth clamped to 1–6."),
            key(
                "text",
                STR,
                ALWAYS,
                "Heading title text with the ATX marker stripped.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.fenced_code_block.v1",
        languages: &["markdown"],
        query_family: "document_structure",
        description: "A Markdown fenced code block.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "language",
                STR,
                OPT,
                "Fence language token (first word of the info string).",
            ),
            key("info_string", STR, OPT, "Full trimmed fence info string."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.inline_link.v1",
        languages: &["markdown"],
        query_family: "document_links",
        description: "A Markdown inline link.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("label", STR, ALWAYS, "Visible link text."),
            key("destination", STR, ALWAYS, "Link target URL/path."),
            key(
                "title",
                STR,
                OPT,
                "Optional link title (only on tree-parsed links, never the regex fallback).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.link_definition.v1",
        languages: &["markdown"],
        query_family: "document_links",
        description: "A Markdown link-reference definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "label",
                STR,
                ALWAYS,
                "Reference label of the link definition.",
            ),
            key(
                "destination",
                STR,
                ALWAYS,
                "Target URL/path the label resolves to.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.table.v1",
        languages: &["markdown"],
        query_family: "document_structure",
        description: "A Markdown pipe table.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "row_count",
                NUM,
                ALWAYS,
                "Total table rows including the header row.",
            ),
            key(
                "column_count",
                NUM,
                ALWAYS,
                "Number of columns detected in the table.",
            ),
            key(
                "header_row",
                STR,
                OPT,
                "Trimmed raw text of the header row, when present.",
            ),
        ],
    },
];

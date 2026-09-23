//! Structural-fact pattern SPECS for the `markdown` registry family.
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries emitted by
//! the markdown arm of `base/data_structural_facts.rs`. Public registry access remains through
//! [`super::structural_fact_pattern_specs`].

use super::{
    ALWAYS, ARR, BOOL, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec,
    key,
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
            key(
                "keys",
                ARR,
                OPT,
                "Top-level frontmatter keys in source order (TOML table names included); absent when none parse.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.heading.v1",
        languages: &["markdown"],
        query_family: "document_structure",
        description: "A Markdown ATX or setext heading with a non-empty title.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("level", NUM, ALWAYS, "Heading depth clamped to 1–6."),
            key(
                "text",
                STR,
                ALWAYS,
                "Heading title as plain text: inline markup, ATX markers, closing hashes, and the `{#id}` attribute block removed.",
            ),
            key(
                "anchor",
                STR,
                OPT,
                "Explicit anchor id from a trailing `{#id}` attribute block.",
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
                "Fence language from the grammar's language token, normalized (`rust,no_run` is `rust`, `{r echo=FALSE}` and `{.r}` are `r`).",
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
    StructuralFactPatternSpec {
        pattern_id: "markdown.reference_link.v1",
        languages: &["markdown"],
        query_family: "document_links",
        description: "A Markdown reference-link usage (`[text][label]`, `[label][]`, or `[label]`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("label", STR, ALWAYS, "Reference label the usage names."),
            key(
                "reference_kind",
                STR,
                ALWAYS,
                "Usage form: \"full\", \"collapsed\", or \"shortcut\".",
            ),
            key(
                "destination",
                STR,
                OPT,
                "Destination of the matching link definition in the same document.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.footnote_reference.v1",
        languages: &["markdown"],
        query_family: "document_links",
        description: "A Markdown footnote reference (`[^label]`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("label", STR, ALWAYS, "Footnote label without the `^`."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.footnote_definition.v1",
        languages: &["markdown"],
        query_family: "document_links",
        description: "A Markdown footnote definition (`[^label]: text`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("label", STR, ALWAYS, "Footnote label without the `^`."),
            key("text", STR, OPT, "Footnote body text."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.autolink.v1",
        languages: &["markdown"],
        query_family: "document_links",
        description: "A Markdown autolink (`<https://...>` or `<name@example.com>`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "destination",
                STR,
                ALWAYS,
                "Autolink target without the angle brackets.",
            ),
            key(
                "autolink_kind",
                STR,
                ALWAYS,
                "Autolink form: \"uri\" or \"email\".",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.task_list_item.v1",
        languages: &["markdown"],
        query_family: "document_structure",
        description: "A Markdown task-list item (`- [ ]` or `- [x]`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "checked",
                BOOL,
                ALWAYS,
                "True when the task marker is checked.",
            ),
            key(
                "text",
                STR,
                ALWAYS,
                "Item paragraph text with whitespace runs collapsed.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.definition_list_item.v1",
        languages: &["markdown"],
        query_family: "document_structure",
        description: "A definition-list item (a term line followed by `: definition`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("term", STR, ALWAYS, "Defined term."),
            key("definition", STR, ALWAYS, "Definition text."),
        ],
    },
];

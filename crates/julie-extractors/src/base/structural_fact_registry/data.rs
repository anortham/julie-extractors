//! Structural-fact pattern SPECS for the `data` registry family.
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries. Public
//! registry access remains through [`super::structural_fact_pattern_specs`].

use super::{
    ALWAYS, BOOL, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    // -----------------------------------------------------------------------
    // Data collector (base/data_structural_facts.rs).
    // -----------------------------------------------------------------------
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
    // JSON
    StructuralFactPatternSpec {
        pattern_id: "json.object.v1",
        languages: &["json"],
        query_family: "data_structure",
        description: "A JSON object node.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "path",
                STR,
                ALWAYS,
                "Dotted/indexed JSON path to this object from the root.",
            ),
            key("depth", NUM, ALWAYS, "Nesting depth of this object."),
            key(
                "property_count",
                NUM,
                ALWAYS,
                "Number of direct properties in the object.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "json.array.v1",
        languages: &["json"],
        query_family: "data_structure",
        description: "A JSON array node.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "path",
                STR,
                ALWAYS,
                "Dotted/indexed JSON path to this array from the root.",
            ),
            key("depth", NUM, ALWAYS, "Nesting depth of this array."),
            key(
                "element_count",
                NUM,
                ALWAYS,
                "Number of elements in the array.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "json.property.v1",
        languages: &["json"],
        query_family: "data_structure",
        description: "A JSON object property.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("key", STR, ALWAYS, "Property key name."),
            key(
                "path",
                STR,
                ALWAYS,
                "Dotted/indexed JSON path to the property's parent.",
            ),
            key(
                "value_kind",
                STR,
                ALWAYS,
                "Normalized kind of the property value.",
            ),
            key("depth", NUM, ALWAYS, "Nesting depth of the property."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "json.schema.v1",
        languages: &["json"],
        query_family: "schema_structure",
        description: "A JSON Schema `$schema` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "schema_uri",
                STR,
                ALWAYS,
                "URI/value of the `$schema` property.",
            ),
            key(
                "path",
                STR,
                ALWAYS,
                "Dotted/indexed JSON path to the property's parent.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "json.ref.v1",
        languages: &["json"],
        query_family: "schema_structure",
        description: "A JSON Schema `$ref` reference.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("ref", STR, ALWAYS, "Target of the `$ref` property."),
            key(
                "path",
                STR,
                ALWAYS,
                "Dotted/indexed JSON path to the property's parent.",
            ),
        ],
    },
    // TOML
    StructuralFactPatternSpec {
        pattern_id: "toml.table.v1",
        languages: &["toml"],
        query_family: "config_structure",
        description: "A TOML `[table]` header.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "table_name",
                STR,
                ALWAYS,
                "Declared name of the `[table]` header.",
            ),
            key(
                "key_path",
                STR,
                ALWAYS,
                "Full dotted key path to the table including ancestors.",
            ),
            key(
                "is_array_table",
                BOOL,
                ALWAYS,
                "Always false for standard tables.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "toml.array_table.v1",
        languages: &["toml"],
        query_family: "config_structure",
        description: "A TOML `[[array table]]` element.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "table_name",
                STR,
                ALWAYS,
                "Declared name of the `[[array_table]]` header.",
            ),
            key(
                "key_path",
                STR,
                ALWAYS,
                "Full dotted key path to the array table including ancestors.",
            ),
            key(
                "is_array_table",
                BOOL,
                ALWAYS,
                "Always true, marking an array-of-tables element.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "toml.key_value.v1",
        languages: &["toml"],
        query_family: "config_structure",
        description: "A TOML key/value assignment.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("key", STR, ALWAYS, "The assignment key name."),
            key(
                "key_path",
                STR,
                ALWAYS,
                "Full dotted key path including the enclosing table path.",
            ),
            key(
                "value_kind",
                STR,
                ALWAYS,
                "Normalized kind of the assigned value.",
            ),
            key(
                "is_array_table",
                BOOL,
                ALWAYS,
                "Always false for key/value pairs.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "toml.inline_table.v1",
        languages: &["toml"],
        query_family: "config_structure",
        description: "A TOML inline table value.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "key_path",
                STR,
                ALWAYS,
                "Full dotted key path of the key holding the inline table.",
            ),
            key(
                "entry_count",
                NUM,
                ALWAYS,
                "Number of direct entries in the inline table.",
            ),
            key(
                "is_array_table",
                BOOL,
                ALWAYS,
                "Always false for inline tables.",
            ),
        ],
    },
    // YAML
    StructuralFactPatternSpec {
        pattern_id: "yaml.document.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML document node.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "has_directives",
                BOOL,
                ALWAYS,
                "Whether the document contains any YAML directive.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.mapping.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML block or flow mapping.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "key_path",
                STR,
                ALWAYS,
                "Dotted key path to this mapping from the document root.",
            ),
            key(
                "pair_count",
                NUM,
                ALWAYS,
                "Number of direct key/value pairs in the mapping.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.sequence.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML block or flow sequence.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "key_path",
                STR,
                ALWAYS,
                "Dotted key path to this sequence from the document root.",
            ),
            key(
                "sequence_length",
                NUM,
                ALWAYS,
                "Number of items in the sequence.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.anchor.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML anchor definition (`&name`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "anchor_name",
                STR,
                ALWAYS,
                "Declared anchor name (the `&name` token).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.alias.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML alias reference (`*name`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "alias_target",
                STR,
                ALWAYS,
                "Target anchor name the alias references (the `*name` token).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.key_value.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML mapping key/value pair.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("key", STR, ALWAYS, "The mapping key name."),
            key(
                "key_path",
                STR,
                ALWAYS,
                "Dotted key path including ancestor keys.",
            ),
            key(
                "value_kind",
                STR,
                ALWAYS,
                "Normalized kind of the mapped value.",
            ),
        ],
    },
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

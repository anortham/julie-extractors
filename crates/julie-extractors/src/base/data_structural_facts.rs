use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::span::NormalizedSpan;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol, stable_location_id};

// Markdown
const MARKDOWN_FRONTMATTER_PATTERN_ID: &str = "markdown.frontmatter.v1";
const MARKDOWN_HEADING_PATTERN_ID: &str = "markdown.heading.v1";
const MARKDOWN_FENCED_CODE_BLOCK_PATTERN_ID: &str = "markdown.fenced_code_block.v1";
const MARKDOWN_LINK_DEFINITION_PATTERN_ID: &str = "markdown.link_definition.v1";
const MARKDOWN_INLINE_LINK_PATTERN_ID: &str = "markdown.inline_link.v1";
const MARKDOWN_TABLE_PATTERN_ID: &str = "markdown.table.v1";

// JSON
const JSON_OBJECT_PATTERN_ID: &str = "json.object.v1";
const JSON_ARRAY_PATTERN_ID: &str = "json.array.v1";
const JSON_PROPERTY_PATTERN_ID: &str = "json.property.v1";

// TOML
const TOML_TABLE_PATTERN_ID: &str = "toml.table.v1";
const TOML_ARRAY_TABLE_PATTERN_ID: &str = "toml.array_table.v1";
const TOML_KEY_VALUE_PATTERN_ID: &str = "toml.key_value.v1";
const TOML_INLINE_TABLE_PATTERN_ID: &str = "toml.inline_table.v1";

// YAML
const YAML_DOCUMENT_PATTERN_ID: &str = "yaml.document.v1";
const YAML_MAPPING_PATTERN_ID: &str = "yaml.mapping.v1";
const YAML_SEQUENCE_PATTERN_ID: &str = "yaml.sequence.v1";
const YAML_ANCHOR_PATTERN_ID: &str = "yaml.anchor.v1";
const YAML_ALIAS_PATTERN_ID: &str = "yaml.alias.v1";
const YAML_KEY_VALUE_PATTERN_ID: &str = "yaml.key_value.v1";

// Regex
const REGEX_CAPTURE_GROUP_PATTERN_ID: &str = "regex.capture_group.v1";
const REGEX_NAMED_CAPTURE_PATTERN_ID: &str = "regex.named_capture.v1";
const REGEX_LOOKAROUND_PATTERN_ID: &str = "regex.lookaround.v1";
const REGEX_CHARACTER_CLASS_PATTERN_ID: &str = "regex.character_class.v1";
const REGEX_QUANTIFIER_PATTERN_ID: &str = "regex.quantifier.v1";
const REGEX_ALTERNATION_PATTERN_ID: &str = "regex.alternation.v1";
const REGEX_ANCHOR_PATTERN_ID: &str = "regex.anchor.v1";

static MARKDOWN_INLINE_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(!)?\[([^\]\n]+)\]\(([^)\n]+)\)").unwrap());

#[cfg(all(test, feature = "test-capability-matrix"))]
const MARKDOWN_DATA_PATTERN_IDS: &[&str] = &[
    MARKDOWN_FENCED_CODE_BLOCK_PATTERN_ID,
    MARKDOWN_FRONTMATTER_PATTERN_ID,
    MARKDOWN_HEADING_PATTERN_ID,
    MARKDOWN_INLINE_LINK_PATTERN_ID,
    MARKDOWN_LINK_DEFINITION_PATTERN_ID,
    MARKDOWN_TABLE_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const JSON_DATA_PATTERN_IDS: &[&str] = &[
    JSON_ARRAY_PATTERN_ID,
    JSON_OBJECT_PATTERN_ID,
    JSON_PROPERTY_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const TOML_DATA_PATTERN_IDS: &[&str] = &[
    TOML_ARRAY_TABLE_PATTERN_ID,
    TOML_INLINE_TABLE_PATTERN_ID,
    TOML_KEY_VALUE_PATTERN_ID,
    TOML_TABLE_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const YAML_DATA_PATTERN_IDS: &[&str] = &[
    YAML_ALIAS_PATTERN_ID,
    YAML_ANCHOR_PATTERN_ID,
    YAML_DOCUMENT_PATTERN_ID,
    YAML_KEY_VALUE_PATTERN_ID,
    YAML_MAPPING_PATTERN_ID,
    YAML_SEQUENCE_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const REGEX_DATA_PATTERN_IDS: &[&str] = &[
    REGEX_ALTERNATION_PATTERN_ID,
    REGEX_ANCHOR_PATTERN_ID,
    REGEX_CAPTURE_GROUP_PATTERN_ID,
    REGEX_CHARACTER_CLASS_PATTERN_ID,
    REGEX_LOOKAROUND_PATTERN_ID,
    REGEX_NAMED_CAPTURE_PATTERN_ID,
    REGEX_QUANTIFIER_PATTERN_ID,
];

pub fn collect_data_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let mut facts = match language {
        "markdown" => collect_markdown_structural_facts(tree, file_path, content),
        "json" => collect_json_structural_facts(tree, file_path, content),
        "toml" => collect_toml_structural_facts(tree, file_path, content),
        "yaml" => collect_yaml_structural_facts(tree, file_path, content),
        "regex" => collect_regex_structural_facts(tree, file_path, content),
        _ => Vec::new(),
    };

    attach_containing_symbols(&mut facts, symbols);
    sort_structural_facts(&mut facts);
    facts
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn data_structural_fact_pattern_ids_for_language(
    language: &str,
) -> &'static [&'static str] {
    match language {
        "markdown" => MARKDOWN_DATA_PATTERN_IDS,
        "json" => JSON_DATA_PATTERN_IDS,
        "toml" => TOML_DATA_PATTERN_IDS,
        "yaml" => YAML_DATA_PATTERN_IDS,
        "regex" => REGEX_DATA_PATTERN_IDS,
        _ => &[],
    }
}

fn collect_markdown_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_markdown_node(tree.root_node(), file_path, content, &mut facts);
    append_markdown_inline_link_facts(file_path, content, &mut facts);
    facts
}

fn append_markdown_inline_link_facts(
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let excluded_spans = markdown_inline_link_excluded_spans(facts);

    for captures in MARKDOWN_INLINE_LINK_RE.captures_iter(content) {
        if captures.get(1).is_some() {
            continue;
        }
        let Some(matched) = captures.get(0) else {
            continue;
        };
        let Some(label) = captures.get(2).map(|matched| matched.as_str()) else {
            continue;
        };
        let Some(destination_match) = captures.get(3) else {
            continue;
        };
        let destination = destination_match.as_str();
        let start = matched.start();
        let end = matched.end();
        if span_is_covered(&excluded_spans, start, end) {
            continue;
        }
        if facts.iter().any(|fact| {
            fact.pattern_id == MARKDOWN_INLINE_LINK_PATTERN_ID
                && fact.start_byte <= start as u32
                && fact.end_byte >= end as u32
        }) {
            continue;
        }

        let Some(span) = NormalizedSpan::from_content_range(content, start, end) else {
            continue;
        };

        let mut metadata = base_metadata("document_links");
        insert_string(&mut metadata, "label", &clean_markdown_link_text(label));
        insert_string(
            &mut metadata,
            "destination",
            &clean_markdown_link_destination(destination),
        );

        facts.push(fact_for_span(
            file_path,
            "markdown",
            MARKDOWN_INLINE_LINK_PATTERN_ID,
            "inline_link",
            "inline_link",
            span,
            metadata,
        ));
    }
}

fn markdown_inline_link_excluded_spans(facts: &[StructuralFact]) -> Vec<(u32, u32)> {
    facts
        .iter()
        .filter(|fact| {
            matches!(
                fact.pattern_id.as_str(),
                MARKDOWN_FENCED_CODE_BLOCK_PATTERN_ID | MARKDOWN_FRONTMATTER_PATTERN_ID
            )
        })
        .map(|fact| (fact.start_byte, fact.end_byte))
        .collect()
}

fn span_is_covered(spans: &[(u32, u32)], start: usize, end: usize) -> bool {
    spans
        .iter()
        .any(|(span_start, span_end)| *span_start <= start as u32 && *span_end >= end as u32)
}

fn collect_markdown_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    match node.kind() {
        "minus_metadata" | "plus_metadata" => {
            if let Some(fact) = markdown_frontmatter_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "atx_heading" | "heading" => {
            if let Some(fact) = markdown_heading_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "fenced_code_block" => {
            if let Some(fact) = markdown_fenced_code_block_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "link_reference_definition" => {
            if let Some(fact) = markdown_link_definition_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "inline_link" => {
            if let Some(fact) = markdown_inline_link_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "pipe_table" | "table" => {
            if let Some(fact) = markdown_table_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_markdown_node(child, file_path, content, facts);
    }
}

fn markdown_frontmatter_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let format = if node.kind() == "plus_metadata" {
        "toml"
    } else {
        "yaml"
    };
    let body = strip_frontmatter_delimiters(text);
    if body.trim().is_empty() {
        return None;
    }

    let key_count = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .count();

    let mut metadata = base_metadata("document_metadata");
    insert_string(&mut metadata, "format", format);
    metadata.insert(
        "key_count".to_string(),
        Value::Number(Number::from(key_count)),
    );

    Some(fact_for_node(
        file_path,
        "markdown",
        MARKDOWN_FRONTMATTER_PATTERN_ID,
        "frontmatter",
        node,
        metadata,
    ))
}

fn markdown_heading_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let level = text.chars().take_while(|ch| *ch == '#').count().clamp(1, 6);
    let heading_text = strip_atx_heading_marker(text);
    if heading_text.is_empty() {
        return None;
    }

    let mut metadata = base_metadata("document_structure");
    metadata.insert("level".to_string(), Value::Number(Number::from(level)));
    insert_string(&mut metadata, "text", &heading_text);

    Some(fact_for_node(
        file_path,
        "markdown",
        MARKDOWN_HEADING_PATTERN_ID,
        "heading",
        node,
        metadata,
    ))
}

fn markdown_fenced_code_block_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let info = child_text(node, content, "info_string")
        .unwrap_or("")
        .trim();
    let language = info.split_whitespace().next().unwrap_or("").trim();
    let mut metadata = base_metadata("document_structure");
    if !language.is_empty() {
        insert_string(&mut metadata, "language", language);
    }
    if !info.is_empty() {
        insert_string(&mut metadata, "info_string", info);
    }

    Some(fact_for_node(
        file_path,
        "markdown",
        MARKDOWN_FENCED_CODE_BLOCK_PATTERN_ID,
        "fenced_code_block",
        node,
        metadata,
    ))
}

fn markdown_inline_link_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let label = clean_markdown_link_text(child_text(node, content, "link_text")?);
    let destination =
        clean_markdown_link_destination(child_text(node, content, "link_destination")?);
    if label.is_empty() || destination.is_empty() {
        return None;
    }

    let mut metadata = base_metadata("document_links");
    insert_string(&mut metadata, "label", &label);
    insert_string(&mut metadata, "destination", &destination);
    if let Some(title) = child_text(node, content, "link_title").map(clean_markdown_link_title)
        && !title.is_empty()
    {
        insert_string(&mut metadata, "title", &title);
    }

    Some(fact_for_node(
        file_path,
        "markdown",
        MARKDOWN_INLINE_LINK_PATTERN_ID,
        "inline_link",
        node,
        metadata,
    ))
}

fn markdown_link_definition_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?.trim();
    let (label, destination) = parse_link_reference_definition(text)?;
    let mut metadata = base_metadata("document_links");
    insert_string(&mut metadata, "label", &label);
    insert_string(&mut metadata, "destination", &destination);

    Some(fact_for_node(
        file_path,
        "markdown",
        MARKDOWN_LINK_DEFINITION_PATTERN_ID,
        "link_definition",
        node,
        metadata,
    ))
}

fn markdown_table_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let row_count = count_direct_children(node, "pipe_table_row")
        + usize::from(has_direct_child(node, "pipe_table_header"))
        + count_direct_children(node, "table_row");
    let column_count = markdown_table_column_count(node);
    let mut metadata = base_metadata("document_structure");
    metadata.insert(
        "row_count".to_string(),
        Value::Number(Number::from(row_count)),
    );
    metadata.insert(
        "column_count".to_string(),
        Value::Number(Number::from(column_count)),
    );
    if let Some(header) = first_child_text(node, content, "pipe_table_header")
        .or_else(|| first_child_text(node, content, "table_header_row"))
        .or_else(|| first_child_text(node, content, "header_row"))
    {
        insert_string(&mut metadata, "header_row", header.trim());
    }

    Some(fact_for_node(
        file_path,
        "markdown",
        MARKDOWN_TABLE_PATTERN_ID,
        "table",
        node,
        metadata,
    ))
}

fn collect_json_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_json_node(tree.root_node(), file_path, content, &[], 0, &mut facts);
    facts
}

fn collect_json_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    path: &[String],
    depth: usize,
    facts: &mut Vec<StructuralFact>,
) {
    match node.kind() {
        "object" => {
            let mut metadata = base_metadata("data_structure");
            insert_string(&mut metadata, "path", &json_path(path));
            metadata.insert("depth".to_string(), Value::Number(Number::from(depth)));
            metadata.insert(
                "property_count".to_string(),
                Value::Number(Number::from(count_direct_children(node, "pair"))),
            );
            facts.push(fact_for_node(
                file_path,
                "json",
                JSON_OBJECT_PATTERN_ID,
                "object",
                node,
                metadata,
            ));
        }
        "array" => {
            let mut metadata = base_metadata("data_structure");
            insert_string(&mut metadata, "path", &json_path(path));
            metadata.insert("depth".to_string(), Value::Number(Number::from(depth)));
            metadata.insert(
                "element_count".to_string(),
                Value::Number(Number::from(count_json_array_elements(node))),
            );
            facts.push(fact_for_node(
                file_path,
                "json",
                JSON_ARRAY_PATTERN_ID,
                "array",
                node,
                metadata,
            ));
        }
        "pair" => {
            if let Some(fact) = json_property_fact(file_path, content, node, path, depth) {
                facts.push(fact);
            }
        }
        _ => {}
    }

    if node.kind() == "pair" {
        let key = json_pair_key(content, node);
        let value_node = json_pair_value(node);
        let mut child_path = path.to_vec();
        if let Some(key) = key {
            child_path.push(key);
        }
        if let Some(value_node) = value_node {
            collect_json_node(
                value_node,
                file_path,
                content,
                &child_path,
                depth + 1,
                facts,
            );
        }
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_json_node(child, file_path, content, path, depth + 1, facts);
        }
    }
}

fn json_property_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
    path: &[String],
    depth: usize,
) -> Option<StructuralFact> {
    let key = json_pair_key(content, node)?;
    let value_node = json_pair_value(node)?;
    let value_kind = json_value_kind(value_node.kind());

    let mut metadata = base_metadata("data_structure");
    insert_string(&mut metadata, "key", &key);
    insert_string(&mut metadata, "path", &json_path(path));
    insert_string(&mut metadata, "value_kind", value_kind);
    metadata.insert("depth".to_string(), Value::Number(Number::from(depth)));

    Some(fact_for_node(
        file_path,
        "json",
        JSON_PROPERTY_PATTERN_ID,
        "property",
        node,
        metadata,
    ))
}

fn collect_toml_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_toml_node(tree.root_node(), file_path, content, &[], &mut facts);
    facts
}

fn collect_toml_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    table_path: &[String],
    facts: &mut Vec<StructuralFact>,
) {
    match node.kind() {
        "table" => {
            if let Some(table_name) = toml_table_name(content, node) {
                let mut metadata = base_metadata("config_structure");
                insert_string(&mut metadata, "table_name", &table_name);
                insert_string(
                    &mut metadata,
                    "key_path",
                    &toml_key_path(table_path, &table_name),
                );
                metadata.insert("is_array_table".to_string(), Value::Bool(false));
                facts.push(fact_for_node(
                    file_path,
                    "toml",
                    TOML_TABLE_PATTERN_ID,
                    "table",
                    node,
                    metadata,
                ));

                let mut child_path = table_path.to_vec();
                child_path.push(table_name);
                walk_toml_children(node, file_path, content, &child_path, facts);
                return;
            }
        }
        "table_array_element" => {
            if let Some(table_name) = toml_table_name(content, node) {
                let mut metadata = base_metadata("config_structure");
                insert_string(&mut metadata, "table_name", &table_name);
                insert_string(
                    &mut metadata,
                    "key_path",
                    &toml_key_path(table_path, &table_name),
                );
                metadata.insert("is_array_table".to_string(), Value::Bool(true));
                facts.push(fact_for_node(
                    file_path,
                    "toml",
                    TOML_ARRAY_TABLE_PATTERN_ID,
                    "array_table",
                    node,
                    metadata,
                ));

                let mut child_path = table_path.to_vec();
                child_path.push(table_name);
                walk_toml_children(node, file_path, content, &child_path, facts);
                return;
            }
        }
        "pair" => {
            if let Some((key_value, inline_table)) =
                toml_key_value_facts(file_path, content, node, table_path)
            {
                facts.push(key_value);
                if let Some(inline_table) = inline_table {
                    facts.push(inline_table);
                    if let (Some(key), Some(value_node)) =
                        (toml_pair_key(content, node), toml_pair_value(node))
                    {
                        let mut inline_path = table_path.to_vec();
                        inline_path.push(key);
                        walk_toml_children(value_node, file_path, content, &inline_path, facts);
                    }
                    return;
                }
            }
        }
        _ => {}
    }

    walk_toml_children(node, file_path, content, table_path, facts);
}

fn walk_toml_children(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    table_path: &[String],
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_toml_node(child, file_path, content, table_path, facts);
    }
}

fn toml_key_value_facts(
    file_path: &str,
    content: &str,
    node: Node<'_>,
    table_path: &[String],
) -> Option<(StructuralFact, Option<StructuralFact>)> {
    let key = toml_pair_key(content, node)?;
    let value_node = toml_pair_value(node)?;
    let key_path = toml_key_path(table_path, &key);

    let mut metadata = base_metadata("config_structure");
    insert_string(&mut metadata, "key", &key);
    insert_string(&mut metadata, "key_path", &key_path);
    insert_string(
        &mut metadata,
        "value_kind",
        toml_value_kind(value_node.kind()),
    );
    metadata.insert("is_array_table".to_string(), Value::Bool(false));

    let key_value = fact_for_node(
        file_path,
        "toml",
        TOML_KEY_VALUE_PATTERN_ID,
        "key_value",
        node,
        metadata,
    );

    let inline_table = (value_node.kind() == "inline_table").then(|| {
        let mut inline_metadata = base_metadata("config_structure");
        insert_string(&mut inline_metadata, "key_path", &key_path);
        inline_metadata.insert(
            "entry_count".to_string(),
            Value::Number(Number::from(count_direct_children(value_node, "pair"))),
        );
        inline_metadata.insert("is_array_table".to_string(), Value::Bool(false));
        fact_for_node(
            file_path,
            "toml",
            TOML_INLINE_TABLE_PATTERN_ID,
            "inline_table",
            value_node,
            inline_metadata,
        )
    });

    Some((key_value, inline_table))
}

fn collect_yaml_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_yaml_node(tree.root_node(), file_path, content, &[], &mut facts);
    facts
}

fn collect_yaml_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    path: &[String],
    facts: &mut Vec<StructuralFact>,
) {
    match node.kind() {
        "document" => {
            let mut metadata = base_metadata("config_structure");
            metadata.insert(
                "has_directives".to_string(),
                Value::Bool(has_child_kind(node, "directive")),
            );
            facts.push(fact_for_node(
                file_path,
                "yaml",
                YAML_DOCUMENT_PATTERN_ID,
                "document",
                node,
                metadata,
            ));
        }
        "block_mapping" => {
            let mut metadata = base_metadata("config_structure");
            insert_string(&mut metadata, "key_path", &yaml_key_path(path));
            metadata.insert(
                "pair_count".to_string(),
                Value::Number(Number::from(count_direct_children(
                    node,
                    "block_mapping_pair",
                ))),
            );
            facts.push(fact_for_node(
                file_path,
                "yaml",
                YAML_MAPPING_PATTERN_ID,
                "mapping",
                node,
                metadata,
            ));
        }
        "block_mapping_pair" => {
            if let Some((key, value_node)) = yaml_pair_key_and_value(content, node) {
                let key_path = yaml_property_path(path, &key);
                let mut metadata = base_metadata("config_structure");
                insert_string(&mut metadata, "key", &key);
                insert_string(&mut metadata, "key_path", &key_path);
                insert_string(
                    &mut metadata,
                    "value_kind",
                    yaml_value_kind(value_node, content),
                );
                facts.push(fact_for_node(
                    file_path,
                    "yaml",
                    YAML_KEY_VALUE_PATTERN_ID,
                    "key_value",
                    node,
                    metadata,
                ));

                let mut child_path = path.to_vec();
                child_path.push(key);
                collect_yaml_node(value_node, file_path, content, &child_path, facts);
                return;
            }
        }
        "block_sequence" => {
            let mut metadata = base_metadata("config_structure");
            metadata.insert(
                "sequence_length".to_string(),
                Value::Number(Number::from(count_direct_children(
                    node,
                    "block_sequence_item",
                ))),
            );
            facts.push(fact_for_node(
                file_path,
                "yaml",
                YAML_SEQUENCE_PATTERN_ID,
                "sequence",
                node,
                metadata,
            ));
        }
        "anchor" => {
            if let Some(name) = first_child_text(node, content, "anchor_name") {
                let mut metadata = base_metadata("config_structure");
                insert_string(&mut metadata, "anchor_name", name.trim());
                facts.push(fact_for_node(
                    file_path,
                    "yaml",
                    YAML_ANCHOR_PATTERN_ID,
                    "anchor",
                    node,
                    metadata,
                ));
            }
        }
        "alias" => {
            if let Some(name) = first_child_text(node, content, "alias_name") {
                let mut metadata = base_metadata("config_structure");
                insert_string(&mut metadata, "alias_target", name.trim());
                facts.push(fact_for_node(
                    file_path,
                    "yaml",
                    YAML_ALIAS_PATTERN_ID,
                    "alias",
                    node,
                    metadata,
                ));
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_yaml_node(child, file_path, content, path, facts);
    }
}

fn collect_regex_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut capture_index = 0usize;
    collect_regex_node(
        tree.root_node(),
        file_path,
        content,
        &mut facts,
        &mut capture_index,
    );
    append_missing_regex_lookaround_facts(file_path, content, &mut facts);
    facts
}

fn append_missing_regex_lookaround_facts(
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let covered_spans = facts
        .iter()
        .filter(|fact| fact.pattern_id == REGEX_LOOKAROUND_PATTERN_ID)
        .map(|fact| (fact.start_byte, fact.end_byte))
        .collect::<Vec<_>>();

    for (lookaround_text, span) in find_regex_lookaround_spans(content) {
        if covered_spans
            .iter()
            .any(|(start, end)| *start <= span.start_byte && *end >= span.end_byte)
        {
            continue;
        }

        let direction = if lookaround_text.contains("(?<=") || lookaround_text.contains("(?<!") {
            "lookbehind"
        } else {
            "lookahead"
        };
        let polarity = if lookaround_text.contains("(?=") || lookaround_text.contains("(?<=") {
            "positive"
        } else {
            "negative"
        };
        let mut metadata = base_metadata("pattern_structure");
        insert_string(&mut metadata, "direction", direction);
        insert_string(&mut metadata, "polarity", polarity);

        facts.push(fact_for_span(
            file_path,
            "regex",
            REGEX_LOOKAROUND_PATTERN_ID,
            "lookaround",
            "lookaround",
            span,
            metadata,
        ));
    }
}

fn find_regex_lookaround_spans(content: &str) -> Vec<(String, NormalizedSpan)> {
    let mut lookarounds = Vec::new();
    let mut index = 0usize;

    while index < content.len() {
        let rest = &content[index..];
        let is_lookaround = rest.starts_with("(?=")
            || rest.starts_with("(?!")
            || rest.starts_with("(?<=")
            || rest.starts_with("(?<!");

        if is_lookaround && let Some(end) = find_regex_group_end(content, index) {
            let end_exclusive = end + 1;
            if let Some(span) = NormalizedSpan::from_content_range(content, index, end_exclusive) {
                lookarounds.push((content[index..end_exclusive].to_string(), span));
            }
            index = end_exclusive;
            continue;
        }

        index += rest
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or_default()
            .max(1);
    }

    lookarounds
}

fn find_regex_group_end(content: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    let mut in_character_class = false;

    for (offset, ch) in content[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '[' if !in_character_class => in_character_class = true,
            ']' if in_character_class => in_character_class = false,
            '(' if !in_character_class => depth += 1,
            ')' if !in_character_class => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }

    None
}

fn collect_regex_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    capture_index: &mut usize,
) {
    match node.kind() {
        "named_capturing_group" => {
            *capture_index += 1;
            if let Some(fact) = regex_named_capture_fact(file_path, content, node, *capture_index) {
                facts.push(fact);
            }
        }
        "anonymous_capturing_group" | "capturing_group" => {
            *capture_index += 1;
            if let Some(fact) = regex_capture_group_fact(file_path, content, node, *capture_index) {
                facts.push(fact);
            }
        }
        "lookahead_assertion"
        | "lookbehind_assertion"
        | "positive_lookahead"
        | "negative_lookahead"
        | "positive_lookbehind"
        | "negative_lookbehind" => {
            if let Some(fact) = regex_lookaround_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "group" | "non_capturing_group" => {
            let text = node_text(content, node).unwrap_or_default();
            if is_lookaround_group_text(text)
                && let Some(fact) = regex_lookaround_fact(file_path, content, node)
            {
                facts.push(fact);
            }
        }
        "character_class" => {
            if let Some(fact) = regex_character_class_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "count_quantifier"
        | "zero_or_more"
        | "one_or_more"
        | "optional"
        | "quantifier"
        | "quantified_expression" => {
            if let Some(fact) = regex_quantifier_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "alternation" | "disjunction" => {
            if let Some(fact) = regex_alternation_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "anchor"
        | "start_assertion"
        | "end_assertion"
        | "word_boundary_assertion"
        | "non_word_boundary_assertion" => {
            if let Some(fact) = regex_anchor_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_regex_node(child, file_path, content, facts, capture_index);
    }
}

fn regex_named_capture_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
    capture_index: usize,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let capture_name = extract_named_capture_name(text)?;
    let mut metadata = base_metadata("pattern_structure");
    insert_string(&mut metadata, "capture_name", &capture_name);
    metadata.insert(
        "capture_index".to_string(),
        Value::Number(Number::from(capture_index)),
    );

    Some(fact_for_node(
        file_path,
        "regex",
        REGEX_NAMED_CAPTURE_PATTERN_ID,
        "named_capture",
        node,
        metadata,
    ))
}

fn regex_capture_group_fact(
    file_path: &str,
    _content: &str,
    node: Node<'_>,
    capture_index: usize,
) -> Option<StructuralFact> {
    let mut metadata = base_metadata("pattern_structure");
    metadata.insert(
        "capture_index".to_string(),
        Value::Number(Number::from(capture_index)),
    );
    metadata.insert("named".to_string(), Value::Bool(false));

    Some(fact_for_node(
        file_path,
        "regex",
        REGEX_CAPTURE_GROUP_PATTERN_ID,
        "capture_group",
        node,
        metadata,
    ))
}

fn regex_lookaround_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let direction = if text.contains("(?<=") || text.contains("(?<!") {
        "lookbehind"
    } else {
        "lookahead"
    };
    let polarity = if text.contains("(?=") || text.contains("(?<=") {
        "positive"
    } else {
        "negative"
    };

    let mut metadata = base_metadata("pattern_structure");
    insert_string(&mut metadata, "direction", direction);
    insert_string(&mut metadata, "polarity", polarity);

    Some(fact_for_node(
        file_path,
        "regex",
        REGEX_LOOKAROUND_PATTERN_ID,
        "lookaround",
        node,
        metadata,
    ))
}

fn regex_character_class_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let mut metadata = base_metadata("pattern_structure");
    metadata.insert(
        "negated".to_string(),
        Value::Bool(text.trim_start().starts_with("[^")),
    );

    Some(fact_for_node(
        file_path,
        "regex",
        REGEX_CHARACTER_CLASS_PATTERN_ID,
        "character_class",
        node,
        metadata,
    ))
}

fn regex_quantifier_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let mut metadata = base_metadata("pattern_structure");
    insert_string(&mut metadata, "quantifier", text.trim());

    Some(fact_for_node(
        file_path,
        "regex",
        REGEX_QUANTIFIER_PATTERN_ID,
        "quantifier",
        node,
        metadata,
    ))
}

fn regex_alternation_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let branch_count = text.matches('|').count().saturating_add(1);
    let mut metadata = base_metadata("pattern_structure");
    metadata.insert(
        "branch_count".to_string(),
        Value::Number(Number::from(branch_count)),
    );

    Some(fact_for_node(
        file_path,
        "regex",
        REGEX_ALTERNATION_PATTERN_ID,
        "alternation",
        node,
        metadata,
    ))
}

fn regex_anchor_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let anchor_kind = match text.trim() {
        "^" => "start",
        "$" => "end",
        r"\b" => "word_boundary",
        r"\B" => "non_word_boundary",
        r"\A" => "string_start",
        r"\Z" => "string_end",
        r"\z" => "absolute_end",
        _ => "other",
    };

    let mut metadata = base_metadata("pattern_structure");
    insert_string(&mut metadata, "anchor_kind", anchor_kind);

    Some(fact_for_node(
        file_path,
        "regex",
        REGEX_ANCHOR_PATTERN_ID,
        "anchor",
        node,
        metadata,
    ))
}

fn fact_for_node(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node: Node<'_>,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    fact_for_span(
        file_path,
        language,
        pattern_id,
        capture_name,
        node.kind(),
        NormalizedSpan::from_node(&node),
        metadata,
    )
}

fn fact_for_span(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node_kind: &str,
    span: NormalizedSpan,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    StructuralFact {
        id: stable_location_id(file_path, &format!("{pattern_id}:{capture_name}"), span),
        file_path: file_path.to_string(),
        language: language.to_string(),
        pattern_id: pattern_id.to_string(),
        capture_name: capture_name.to_string(),
        node_kind: node_kind.to_string(),
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    }
}

fn base_metadata(query_family: &str) -> HashMap<String, Value> {
    HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String(query_family.to_string()),
        ),
    ])
}

fn insert_string(metadata: &mut HashMap<String, Value>, key: &str, value: &str) {
    metadata.insert(key.to_string(), Value::String(value.to_string()));
}

fn attach_containing_symbols(facts: &mut [StructuralFact], symbols: &[Symbol]) {
    for fact in facts {
        fact.containing_symbol_id = symbols
            .iter()
            .filter(|symbol| {
                symbol.start_byte <= fact.start_byte && symbol.end_byte >= fact.end_byte
            })
            .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
            .map(|symbol| symbol.id.clone());
    }
}

fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

fn child_text<'a>(node: Node<'_>, content: &'a str, child_kind: &str) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == child_kind {
            return node_text(content, child);
        }
    }
    None
}

fn first_child_text<'a>(node: Node<'_>, content: &'a str, child_kind: &str) -> Option<&'a str> {
    child_text(node, content, child_kind)
}

fn count_direct_children(node: Node<'_>, child_kind: &str) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.kind() == child_kind)
        .count()
}

fn has_direct_child(node: Node<'_>, child_kind: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == child_kind)
}

fn markdown_table_column_count(node: Node<'_>) -> usize {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "pipe_table_header" | "pipe_table_row" | "table_row"
        ) {
            return count_direct_children(child, "pipe_table_cell")
                .max(count_direct_children(child, "table_cell"));
        }
    }
    0
}

fn has_child_kind(node: Node<'_>, child_kind: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == child_kind || has_child_kind(child, child_kind) {
            return true;
        }
    }
    false
}

fn strip_frontmatter_delimiters(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 2 {
        return String::new();
    }
    let start = 1;
    let end = if matches!(
        lines.last().map(|line| line.trim()),
        Some("---") | Some("+++")
    ) {
        lines.len().saturating_sub(1)
    } else {
        lines.len()
    };
    lines.get(start..end).unwrap_or(&[]).join("\n")
}

fn strip_atx_heading_marker(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let marker_len = trimmed.chars().take_while(|ch| *ch == '#').count();
    if marker_len == 0 {
        return trimmed.trim().to_string();
    }
    trimmed[marker_len.min(6)..]
        .trim_start()
        .trim_end()
        .to_string()
}

fn clean_markdown_link_text(raw: &str) -> String {
    raw.trim().to_string()
}

fn clean_markdown_link_destination(raw: &str) -> String {
    raw.trim_matches(|ch| ch == '<' || ch == '>' || ch == '(' || ch == ')')
        .trim()
        .to_string()
}

fn clean_markdown_link_title(raw: &str) -> String {
    raw.trim_matches(|ch| ch == '"' || ch == '\'' || ch == '(' || ch == ')')
        .trim()
        .to_string()
}

fn yaml_key_path(path: &[String]) -> String {
    if path.is_empty() {
        "$".to_string()
    } else {
        format!("$.{}", path.join("."))
    }
}

fn yaml_property_path(path: &[String], key: &str) -> String {
    if path.is_empty() {
        format!("$.{key}")
    } else {
        format!("$.{}.{}", path.join("."), key)
    }
}

fn yaml_pair_key_and_value<'a>(content: &str, node: Node<'a>) -> Option<(String, Node<'a>)> {
    let mut cursor = node.walk();
    let mut key = None;
    for child in node.children(&mut cursor) {
        match child.kind() {
            "flow_node" | "block_node" => {
                if key.is_none() {
                    key = yaml_node_scalar_text(content, child);
                } else {
                    return Some((key?, child));
                }
            }
            _ => {}
        }
    }
    None
}

fn yaml_node_scalar_text(content: &str, node: Node<'_>) -> Option<String> {
    if matches!(
        node.kind(),
        "plain_scalar" | "double_quote_scalar" | "single_quote_scalar"
    ) {
        let text = node_text(content, node)?;
        return Some(text.trim_matches('"').trim_matches('\'').trim().to_string());
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(text) = yaml_node_scalar_text(content, child) {
            return Some(text);
        }
    }
    None
}

fn yaml_value_kind(value_node: Node<'_>, content: &str) -> &'static str {
    match value_node.kind() {
        "block_node" => {
            let mut cursor = value_node.walk();
            if let Some(child) = value_node.children(&mut cursor).next() {
                match child.kind() {
                    "block_mapping" => "mapping",
                    "block_sequence" => "sequence",
                    "alias" => "alias",
                    "anchor" => "anchor",
                    _ => "other",
                }
            } else {
                "other"
            }
        }
        "flow_node" => {
            let mut cursor = value_node.walk();
            if let Some(child) = value_node.children(&mut cursor).next() {
                match child.kind() {
                    "plain_scalar" | "double_quote_scalar" | "single_quote_scalar" => "scalar",
                    "flow_mapping" => "mapping",
                    "flow_sequence" => "sequence",
                    "alias" => "alias",
                    "anchor" => "anchor",
                    _ => "other",
                }
            } else {
                yaml_node_scalar_text(content, value_node)
                    .map(|_| "scalar")
                    .unwrap_or("other")
            }
        }
        _ => "other",
    }
}

fn parse_link_reference_definition(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let (label_part, rest) = trimmed.split_once("]:")?;
    let label = label_part.trim_start_matches('[').trim().to_string();
    let destination = rest.split_whitespace().next()?.to_string();
    (!label.is_empty() && !destination.is_empty()).then_some((label, destination))
}

fn json_path(path: &[String]) -> String {
    if path.is_empty() {
        "$".to_string()
    } else {
        format!("$.{}", path.join("."))
    }
}

fn json_pair_key(content: &str, node: Node<'_>) -> Option<String> {
    let key_node = node.child(0)?;
    let text = node_text(content, key_node)?;
    Some(text.trim_matches('"').to_string())
}

fn json_pair_value(node: Node<'_>) -> Option<Node<'_>> {
    let index = node.child_count().saturating_sub(1) as u32;
    node.child(index)
}

fn json_value_kind(kind: &str) -> &'static str {
    match kind {
        "object" => "object",
        "array" => "array",
        "string" => "string",
        "number" => "number",
        "true" | "false" => "boolean",
        "null" => "null",
        _ => "other",
    }
}

fn count_json_array_elements(node: Node<'_>) -> usize {
    let mut count = 0usize;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "object" | "array" | "string" | "number" | "true" | "false" | "null"
        ) {
            count += 1;
        }
    }
    count
}

fn toml_table_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "bare_key" | "quoted_key" | "dotted_key" => {
                let name = node_text(content, child)?;
                return Some(name.trim_matches('"').trim_matches('\'').to_string());
            }
            _ => {
                if let Some(name) = toml_table_name(content, child) {
                    return Some(name);
                }
            }
        }
    }
    None
}

fn toml_pair_key(content: &str, node: Node<'_>) -> Option<String> {
    let key_node = node.child(0)?;
    let text = match key_node.kind() {
        "bare_key" | "dotted_key" => node_text(content, key_node)?,
        "quoted_key" => {
            let raw = node_text(content, key_node)?;
            return Some(raw.trim_matches('"').trim_matches('\'').to_string());
        }
        _ => return None,
    };
    Some(text.to_string())
}

fn toml_pair_value(node: Node<'_>) -> Option<Node<'_>> {
    let index = node.child_count().saturating_sub(1) as u32;
    node.child(index)
}

fn toml_value_kind(kind: &str) -> &'static str {
    match kind {
        "string" => "string",
        "integer" | "float" => "number",
        "boolean" => "boolean",
        "array" => "array",
        "inline_table" => "inline_table",
        "table" | "table_array_element" => "table",
        "date" | "time" | "offset_date_time" | "local_date_time" | "local_date" | "local_time" => {
            "datetime"
        }
        _ => "other",
    }
}

fn toml_key_path(table_path: &[String], key: &str) -> String {
    if table_path.is_empty() {
        key.to_string()
    } else {
        format!("{}.{}", table_path.join("."), key)
    }
}

fn extract_named_capture_name(text: &str) -> Option<String> {
    if let Some(start) = text.find("(?<")
        && let Some(end) = text[start + 3..].find('>')
    {
        let name = &text[start + 3..start + 3 + end];
        return (!name.is_empty()).then(|| name.to_string());
    }
    if let Some(start) = text.find("(?P<")
        && let Some(end) = text[start + 4..].find('>')
    {
        let name = &text[start + 4..start + 4 + end];
        return (!name.is_empty()).then(|| name.to_string());
    }
    None
}

fn is_lookaround_group_text(group_text: &str) -> bool {
    group_text.starts_with("(?=")
        || group_text.starts_with("(?!")
        || group_text.starts_with("(?<=")
        || group_text.starts_with("(?<!")
}

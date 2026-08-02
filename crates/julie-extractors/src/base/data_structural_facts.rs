use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::attach_containing_symbols;
use super::span::NormalizedSpan;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol, stable_location_id};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

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
const JSON_SCHEMA_PATTERN_ID: &str = "json.schema.v1";
const JSON_REF_PATTERN_ID: &str = "json.ref.v1";

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

// XML
const XML_DOCUMENT_PATTERN_ID: &str = "xml.document.v1";
const XML_NAMESPACE_DECLARATION_PATTERN_ID: &str = "xml.namespace_declaration.v1";
const XML_XSD_TYPE_PATTERN_ID: &str = "xml.xsd.type.v1";
const XML_XSD_ELEMENT_PATTERN_ID: &str = "xml.xsd.element.v1";
const XML_XSD_IMPORT_PATTERN_ID: &str = "xml.xsd.import.v1";
const XML_WSDL_SERVICE_PATTERN_ID: &str = "xml.wsdl.service.v1";
const XML_WSDL_PORT_PATTERN_ID: &str = "xml.wsdl.port.v1";
const XML_WSDL_BINDING_PATTERN_ID: &str = "xml.wsdl.binding.v1";
const XML_WSDL_MESSAGE_PATTERN_ID: &str = "xml.wsdl.message.v1";
const XML_WSDL_OPERATION_PATTERN_ID: &str = "xml.wsdl.operation.v1";

// Regex
const REGEX_CAPTURE_GROUP_PATTERN_ID: &str = "regex.capture_group.v1";
const REGEX_NAMED_CAPTURE_PATTERN_ID: &str = "regex.named_capture.v1";
const REGEX_LOOKAROUND_PATTERN_ID: &str = "regex.lookaround.v1";
const REGEX_CHARACTER_CLASS_PATTERN_ID: &str = "regex.character_class.v1";
const REGEX_QUANTIFIER_PATTERN_ID: &str = "regex.quantifier.v1";
const REGEX_ALTERNATION_PATTERN_ID: &str = "regex.alternation.v1";
const REGEX_ANCHOR_PATTERN_ID: &str = "regex.anchor.v1";

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
    JSON_REF_PATTERN_ID,
    JSON_SCHEMA_PATTERN_ID,
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
const XML_DATA_PATTERN_IDS: &[&str] = &[
    XML_DOCUMENT_PATTERN_ID,
    XML_NAMESPACE_DECLARATION_PATTERN_ID,
    XML_WSDL_BINDING_PATTERN_ID,
    XML_WSDL_MESSAGE_PATTERN_ID,
    XML_WSDL_OPERATION_PATTERN_ID,
    XML_WSDL_PORT_PATTERN_ID,
    XML_WSDL_SERVICE_PATTERN_ID,
    XML_XSD_ELEMENT_PATTERN_ID,
    XML_XSD_IMPORT_PATTERN_ID,
    XML_XSD_TYPE_PATTERN_ID,
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
        "xml" => collect_xml_structural_facts(tree, file_path, content),
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
        "xml" => XML_DATA_PATTERN_IDS,
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
    collect_markdown_node(tree.root_node(), file_path, content, &mut facts, 0);
    append_markdown_setext_heading_facts(file_path, content, &mut facts);
    append_markdown_inline_link_facts(file_path, content, &mut facts);
    facts
}

fn append_markdown_inline_link_facts(
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let excluded_spans = markdown_inline_link_excluded_spans(content, facts);

    for link in find_markdown_inline_links(content) {
        let start = link.start;
        let end = link.end;
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
        insert_string(
            &mut metadata,
            "label",
            &clean_markdown_link_text(&link.label),
        );
        insert_string(
            &mut metadata,
            "destination",
            &clean_markdown_link_destination(&link.destination),
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

fn append_markdown_setext_heading_facts(
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let excluded_spans = markdown_block_excluded_spans(facts);
    let mut previous: Option<(usize, usize, &str)> = None;
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        let line_start = offset;
        let line_end = offset + line.len();
        let trimmed = line.trim();
        if let Some(level) = setext_heading_level(trimmed)
            && let Some((heading_start, heading_end, heading_text)) = previous
            && !heading_text.trim().is_empty()
            && !span_is_covered(&excluded_spans, heading_start, line_end)
            && !facts.iter().any(|fact| {
                fact.pattern_id == MARKDOWN_HEADING_PATTERN_ID
                    && fact.start_byte <= heading_start as u32
                    && fact.end_byte >= heading_end as u32
            })
            && let Some(span) = NormalizedSpan::from_content_range(content, heading_start, line_end)
        {
            let mut metadata = base_metadata("document_structure");
            metadata.insert("level".to_string(), Value::Number(Number::from(level)));
            insert_string(&mut metadata, "text", heading_text.trim());
            facts.push(fact_for_span(
                file_path,
                "markdown",
                MARKDOWN_HEADING_PATTERN_ID,
                "heading",
                "setext_heading",
                span,
                metadata,
            ));
        }
        previous = if trimmed.is_empty() {
            None
        } else {
            Some((
                line_start,
                line_end,
                line.trim_end_matches('\n').trim_end_matches('\r'),
            ))
        };
        offset = line_end;
    }
}

fn setext_heading_level(line: &str) -> Option<u64> {
    if line.len() < 3 {
        return None;
    }
    if line.bytes().all(|byte| byte == b'=') {
        return Some(1);
    }
    if line.bytes().all(|byte| byte == b'-') {
        return Some(2);
    }
    None
}

fn markdown_inline_link_excluded_spans(content: &str, facts: &[StructuralFact]) -> Vec<(u32, u32)> {
    let mut spans = markdown_block_excluded_spans(facts);
    spans.extend(markdown_inline_code_spans(content));
    spans
}

fn markdown_block_excluded_spans(facts: &[StructuralFact]) -> Vec<(u32, u32)> {
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

struct MarkdownInlineLink {
    start: usize,
    end: usize,
    label: String,
    destination: String,
}

fn find_markdown_inline_links(content: &str) -> Vec<MarkdownInlineLink> {
    let bytes = content.as_bytes();
    let mut links = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] != b'[' || cursor > 0 && bytes[cursor - 1] == b'!' {
            cursor += 1;
            continue;
        }
        let Some((label, close_bracket)) = parse_markdown_link_label(content, cursor) else {
            cursor += 1;
            continue;
        };
        if bytes.get(close_bracket + 1) != Some(&b'(') {
            cursor += 1;
            continue;
        }
        let Some((destination, close_paren)) =
            parse_markdown_link_destination(content, close_bracket + 1)
        else {
            cursor += 1;
            continue;
        };
        links.push(MarkdownInlineLink {
            start: cursor,
            end: close_paren + 1,
            label,
            destination,
        });
        cursor = close_paren + 1;
    }
    links
}

fn parse_markdown_link_label(content: &str, open: usize) -> Option<(String, usize)> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (relative, ch) in content[open..].char_indices() {
        let index = open + relative;
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\n' | '\r' => return None,
            '[' => depth += 1,
            ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((content[open + 1..index].to_string(), index));
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_markdown_link_destination(content: &str, open: usize) -> Option<(String, usize)> {
    let mut escaped = false;
    for (relative, ch) in content[open + 1..].char_indices() {
        let index = open + 1 + relative;
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\n' | '\r' => return None,
            ')' => return Some((content[open + 1..index].to_string(), index)),
            _ => {}
        }
    }
    None
}

fn markdown_inline_code_spans(content: &str) -> Vec<(u32, u32)> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] != b'`' {
            cursor += 1;
            continue;
        }
        let tick_count = bytes[cursor..]
            .iter()
            .take_while(|byte| **byte == b'`')
            .count();
        let closing = "`".repeat(tick_count);
        let search_start = cursor + tick_count;
        let Some(relative_close) = content[search_start..].find(&closing) else {
            break;
        };
        let end = search_start + relative_close + tick_count;
        spans.push((cursor as u32, end as u32));
        cursor = end;
    }
    spans
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
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

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

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_markdown_node(child, file_path, content, facts, child_depth);
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

    let key_count = count_frontmatter_keys(&body, format);

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

fn count_frontmatter_keys(body: &str, format: &str) -> usize {
    body.lines()
        .filter(|line| match format {
            "toml" => toml_frontmatter_key_line(line),
            _ => yaml_frontmatter_key_line(line),
        })
        .count()
}

fn yaml_frontmatter_key_line(line: &str) -> bool {
    let trimmed = line.trim_end();
    if trimmed.is_empty()
        || trimmed.trim_start().starts_with('#')
        || line.chars().next().is_some_and(char::is_whitespace)
        || trimmed.starts_with('-')
    {
        return false;
    }
    trimmed
        .split_once(':')
        .is_some_and(|(key, _)| !key.trim().is_empty())
}

fn toml_frontmatter_key_line(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('#')
        && !trimmed.starts_with('[')
        && trimmed.contains('=')
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
    collect_json_node(tree.root_node(), file_path, content, &[], 0, &mut facts, 0);
    facts
}

fn collect_json_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    path: &[String],
    depth: usize,
    facts: &mut Vec<StructuralFact>,
    traversal_depth: u32,
) {
    if !should_visit_tree_depth(traversal_depth) {
        return;
    }

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
            if let Some(fact) = json_schema_or_ref_fact(file_path, content, node, path) {
                facts.push(fact);
            }
        }
        _ => {}
    }

    let Some(child_traversal_depth) = child_tree_depth(traversal_depth) else {
        return;
    };
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
                child_traversal_depth,
            );
        }
    } else if node.kind() == "array" {
        let mut cursor = node.walk();
        let mut index = 0usize;
        for child in node.children(&mut cursor) {
            if !is_json_value_node_kind(child.kind()) {
                continue;
            }
            let mut child_path = path.to_vec();
            child_path.push(format!("[{index}]"));
            collect_json_node(
                child,
                file_path,
                content,
                &child_path,
                depth + 1,
                facts,
                child_traversal_depth,
            );
            index += 1;
        }
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_json_node(
                child,
                file_path,
                content,
                path,
                depth + 1,
                facts,
                child_traversal_depth,
            );
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

fn json_schema_or_ref_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
    path: &[String],
) -> Option<StructuralFact> {
    let key = json_pair_key(content, node)?;
    let value_node = json_pair_value(node)?;
    if value_node.kind() != "string" {
        return None;
    }
    let value = serde_json::from_str::<String>(node_text(content, value_node)?.trim()).ok()?;
    if value.is_empty() {
        return None;
    }

    let (pattern_id, capture_name, value_key) = match key.as_str() {
        "$schema" => (JSON_SCHEMA_PATTERN_ID, "schema", "schema_uri"),
        "$ref" => (JSON_REF_PATTERN_ID, "ref", "ref"),
        _ => return None,
    };

    let mut metadata = base_metadata("schema_structure");
    insert_string(&mut metadata, value_key, &value);
    insert_string(&mut metadata, "path", &json_path(path));

    Some(fact_for_node(
        file_path,
        "json",
        pattern_id,
        capture_name,
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
    collect_toml_node(tree.root_node(), file_path, content, &[], &mut facts, 0);
    facts
}

fn collect_toml_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    table_path: &[String],
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

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
                walk_toml_children(node, file_path, content, &child_path, facts, depth);
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
                walk_toml_children(node, file_path, content, &child_path, facts, depth);
                return;
            }
        }
        "pair" => {
            let effective_table_path = toml_inline_array_table_path(content, node, table_path)
                .unwrap_or_else(|| table_path.to_vec());
            if let Some((key_value, inline_table)) =
                toml_key_value_facts(file_path, content, node, &effective_table_path)
            {
                facts.push(key_value);
                if let Some(value_node) = toml_pair_value(node) {
                    if let Some(inline_table) = inline_table {
                        facts.push(inline_table);
                        if let Some(key_parts) = toml_pair_key_parts(content, node) {
                            let mut inline_path = table_path.to_vec();
                            inline_path.extend(key_parts);
                            walk_toml_children(
                                value_node,
                                file_path,
                                content,
                                &inline_path,
                                facts,
                                depth,
                            );
                            return;
                        }
                    }
                    if value_node.kind() == "array"
                        && let Some(key_parts) = toml_pair_key_parts(content, node)
                    {
                        let mut array_path = table_path.to_vec();
                        array_path.extend(key_parts);
                        walk_toml_children(
                            value_node,
                            file_path,
                            content,
                            &array_path,
                            facts,
                            depth,
                        );
                        return;
                    }
                }
            }
        }
        "inline_table" => {
            if let Some(inline_path) = toml_inline_array_table_path(content, node, table_path) {
                let key_path = toml_render_path(&inline_path);
                let mut inline_metadata = base_metadata("config_structure");
                insert_string(&mut inline_metadata, "key_path", &key_path);
                inline_metadata.insert(
                    "entry_count".to_string(),
                    Value::Number(Number::from(count_direct_children(node, "pair"))),
                );
                inline_metadata.insert("is_array_table".to_string(), Value::Bool(false));
                facts.push(fact_for_node(
                    file_path,
                    "toml",
                    TOML_INLINE_TABLE_PATTERN_ID,
                    "inline_table",
                    node,
                    inline_metadata,
                ));
                walk_toml_children(node, file_path, content, &inline_path, facts, depth);
                return;
            }
        }
        "array" => {
            collect_toml_array_children(node, file_path, content, table_path, facts, depth);
            return;
        }
        _ => {}
    }

    walk_toml_children(node, file_path, content, table_path, facts, depth);
}

fn walk_toml_children(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    table_path: &[String],
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_toml_node(child, file_path, content, table_path, facts, child_depth);
    }
}

fn collect_toml_array_children(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    table_path: &[String],
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    let mut index = 0usize;
    for child in node.children(&mut cursor) {
        if child.kind() != "inline_table" {
            continue;
        }
        let mut indexed_path = table_path.to_vec();
        indexed_path.push(format!("[{index}]"));
        let key_path = toml_render_path(&indexed_path);
        let mut inline_metadata = base_metadata("config_structure");
        insert_string(&mut inline_metadata, "key_path", &key_path);
        inline_metadata.insert(
            "entry_count".to_string(),
            Value::Number(Number::from(count_direct_children(child, "pair"))),
        );
        inline_metadata.insert("is_array_table".to_string(), Value::Bool(false));
        facts.push(fact_for_node(
            file_path,
            "toml",
            TOML_INLINE_TABLE_PATTERN_ID,
            "inline_table",
            child,
            inline_metadata,
        ));
        walk_toml_children(child, file_path, content, &indexed_path, facts, child_depth);
        index += 1;
    }
}

fn toml_inline_array_table_path(
    content: &str,
    node: Node<'_>,
    table_path: &[String],
) -> Option<Vec<String>> {
    if table_path
        .last()
        .is_some_and(|segment| segment.starts_with('['))
    {
        return None;
    }
    let inline_table = if node.kind() == "inline_table" {
        node
    } else {
        ancestor_of_toml_kind(node, "inline_table")?
    };
    let array = ancestor_of_toml_kind(inline_table, "array")?;
    let owner_pair = ancestor_of_toml_kind(array, "pair")?;
    let owner_key_parts = toml_pair_key_parts(content, owner_pair)?;
    let mut path = table_path.to_vec();
    if !path.ends_with(&owner_key_parts) {
        path.extend(owner_key_parts);
    }
    let index = toml_inline_table_index(array, inline_table)?;
    path.push(format!("[{index}]"));
    Some(path)
}

fn ancestor_of_toml_kind<'a>(mut node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return Some(parent);
        }
        node = parent;
    }
    None
}

fn toml_inline_table_index(array: Node<'_>, target: Node<'_>) -> Option<usize> {
    let mut index = 0usize;
    toml_inline_table_index_inner(array, target, 0, &mut index)
}

fn toml_inline_table_index_inner(
    node: Node<'_>,
    target: Node<'_>,
    depth: u32,
    index: &mut usize,
) -> Option<usize> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    if node.kind() == "inline_table" {
        if same_toml_node(node, target) {
            return Some(*index);
        }
        *index += 1;
        return None;
    }
    let child_depth = child_tree_depth(depth)?;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = toml_inline_table_index_inner(child, target, child_depth, index) {
            return Some(found);
        }
    }
    None
}

fn same_toml_node(left: Node<'_>, right: Node<'_>) -> bool {
    left.start_byte() == right.start_byte() && left.end_byte() == right.end_byte()
}

fn toml_key_value_facts(
    file_path: &str,
    content: &str,
    node: Node<'_>,
    table_path: &[String],
) -> Option<(StructuralFact, Option<StructuralFact>)> {
    let key_parts = toml_pair_key_parts(content, node)?;
    let key = key_parts.last()?.clone();
    let value_node = toml_pair_value(node)?;
    let key_path = toml_key_path_parts(table_path, &key_parts);

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
    collect_yaml_node(tree.root_node(), file_path, content, &[], &mut facts, 0);
    facts
}

fn collect_yaml_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    path: &[String],
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

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
        "block_mapping" | "flow_mapping" => {
            let mut metadata = base_metadata("config_structure");
            insert_string(&mut metadata, "key_path", &yaml_key_path(path));
            metadata.insert(
                "pair_count".to_string(),
                Value::Number(Number::from(yaml_pair_count(node))),
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
        "block_mapping_pair" | "flow_pair" | "flow_mapping_pair" => {
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
                if let Some(child_depth) = child_tree_depth(depth) {
                    collect_yaml_node(
                        value_node,
                        file_path,
                        content,
                        &child_path,
                        facts,
                        child_depth,
                    );
                }
                return;
            }
        }
        "block_sequence" | "flow_sequence" => {
            let mut metadata = base_metadata("config_structure");
            insert_string(&mut metadata, "key_path", &yaml_key_path(path));
            metadata.insert(
                "sequence_length".to_string(),
                Value::Number(Number::from(yaml_sequence_length(node))),
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

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_yaml_node(child, file_path, content, path, facts, child_depth);
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
        0,
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
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

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

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_regex_node(child, file_path, content, facts, capture_index, child_depth);
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

/// Which fact layers apply to a document, chosen by registered extension.
#[derive(Clone, Copy, PartialEq, Eq)]
enum XmlDialect {
    Document,
    Schema,
    Service,
}

impl XmlDialect {
    fn label(self) -> &'static str {
        match self {
            XmlDialect::Document => "xml",
            XmlDialect::Schema => "xsd",
            XmlDialect::Service => "wsdl",
        }
    }
}

/// The per-document constants every xml collector step needs.
struct XmlDocument<'a> {
    file_path: &'a str,
    content: &'a str,
    dialect: XmlDialect,
}

#[derive(Default)]
struct XmlDocumentStats {
    root_element: Option<String>,
    has_xml_declaration: bool,
    element_count: u64,
    max_depth: u64,
    namespace_count: u64,
}

fn xml_dialect(file_path: &str) -> XmlDialect {
    match std::path::Path::new(file_path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if extension.eq_ignore_ascii_case("xsd") => XmlDialect::Schema,
        Some(extension) if extension.eq_ignore_ascii_case("wsdl") => XmlDialect::Service,
        _ => XmlDialect::Document,
    }
}

fn collect_xml_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let document = XmlDocument {
        file_path,
        content,
        dialect: xml_dialect(file_path),
    };
    let mut facts = Vec::new();
    let mut stats = XmlDocumentStats::default();
    let root = tree.root_node();
    collect_xml_node(root, &document, &mut facts, &mut stats, 0, 1);

    if let Some(root_element) = stats.root_element.as_deref() {
        let mut metadata = base_metadata("document_structure");
        insert_string(&mut metadata, "dialect", document.dialect.label());
        insert_string(&mut metadata, "root_element", root_element);
        metadata.insert(
            "has_xml_declaration".to_string(),
            Value::Bool(stats.has_xml_declaration),
        );
        metadata.insert(
            "element_count".to_string(),
            Value::Number(Number::from(stats.element_count)),
        );
        metadata.insert(
            "max_depth".to_string(),
            Value::Number(Number::from(stats.max_depth)),
        );
        metadata.insert(
            "namespace_count".to_string(),
            Value::Number(Number::from(stats.namespace_count)),
        );
        facts.push(fact_for_node(
            file_path,
            "xml",
            XML_DOCUMENT_PATTERN_ID,
            "document",
            root,
            metadata,
        ));
    }

    facts
}

fn collect_xml_node(
    node: Node<'_>,
    document: &XmlDocument<'_>,
    facts: &mut Vec<StructuralFact>,
    stats: &mut XmlDocumentStats,
    depth: u32,
    element_depth: u64,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let mut child_element_depth = element_depth;
    match node.kind() {
        "XMLDecl" => stats.has_xml_declaration = true,
        "element" => {
            stats.element_count += 1;
            stats.max_depth = stats.max_depth.max(element_depth);
            child_element_depth = element_depth + 1;
            if stats.root_element.is_none()
                && let Some(name) = xml_element_tag_name(node, document.content)
            {
                stats.root_element = Some(name.to_string());
            }
            collect_xml_element_facts(node, document, facts);
        }
        "Attribute" => {
            if let Some(fact) =
                xml_namespace_declaration_fact(node, document.file_path, document.content)
            {
                stats.namespace_count += 1;
                facts.push(fact);
            }
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_xml_node(
            child,
            document,
            facts,
            stats,
            child_depth,
            child_element_depth,
        );
    }
}

fn collect_xml_element_facts(
    element: Node<'_>,
    document: &XmlDocument<'_>,
    facts: &mut Vec<StructuralFact>,
) {
    let (file_path, content, dialect) = (document.file_path, document.content, document.dialect);
    let Some(local_name) = xml_element_tag_name(element, content).map(xml_local_name) else {
        return;
    };

    match dialect {
        XmlDialect::Document => {}
        XmlDialect::Schema => match local_name {
            "complexType" | "simpleType" => {
                let type_kind = if local_name == "simpleType" {
                    "simple"
                } else {
                    "complex"
                };
                if let Some(type_name) = xml_element_attribute(element, content, "name") {
                    let mut metadata = base_metadata("schema_structure");
                    insert_string(&mut metadata, "type_name", &type_name);
                    insert_string(&mut metadata, "type_kind", type_kind);
                    if let Some(base_type) = xsd_base_type(element, content, 0) {
                        insert_string(&mut metadata, "base_type", &base_type);
                    }
                    facts.push(fact_for_node(
                        file_path,
                        "xml",
                        XML_XSD_TYPE_PATTERN_ID,
                        "type",
                        element,
                        metadata,
                    ));
                }
            }
            "element" => {
                let is_top_level = xml_parent_element(element)
                    .and_then(|parent| xml_element_tag_name(parent, content))
                    .map(xml_local_name)
                    == Some("schema");
                if is_top_level
                    && let Some(element_name) = xml_element_attribute(element, content, "name")
                {
                    let mut metadata = base_metadata("schema_structure");
                    insert_string(&mut metadata, "element_name", &element_name);
                    if let Some(type_ref) = xml_element_attribute(element, content, "type") {
                        insert_string(&mut metadata, "type_ref", &type_ref);
                    }
                    facts.push(fact_for_node(
                        file_path,
                        "xml",
                        XML_XSD_ELEMENT_PATTERN_ID,
                        "element",
                        element,
                        metadata,
                    ));
                }
            }
            "import" | "include" => {
                let mut metadata = base_metadata("schema_structure");
                insert_string(&mut metadata, "import_kind", local_name);
                if let Some(schema_location) =
                    xml_element_attribute(element, content, "schemaLocation")
                {
                    insert_string(&mut metadata, "schema_location", &schema_location);
                }
                if let Some(namespace) = xml_element_attribute(element, content, "namespace") {
                    insert_string(&mut metadata, "namespace", &namespace);
                }
                facts.push(fact_for_node(
                    file_path,
                    "xml",
                    XML_XSD_IMPORT_PATTERN_ID,
                    "import",
                    element,
                    metadata,
                ));
            }
            _ => {}
        },
        XmlDialect::Service => match local_name {
            "service" => {
                if let Some(service_name) = xml_element_attribute(element, content, "name") {
                    let mut metadata = base_metadata("service_structure");
                    insert_string(&mut metadata, "service_name", &service_name);
                    metadata.insert(
                        "port_count".to_string(),
                        Value::Number(Number::from(xml_child_element_count(
                            element, content, "port",
                        ))),
                    );
                    facts.push(fact_for_node(
                        file_path,
                        "xml",
                        XML_WSDL_SERVICE_PATTERN_ID,
                        "service",
                        element,
                        metadata,
                    ));
                }
            }
            "port" => {
                if let Some(port_name) = xml_element_attribute(element, content, "name") {
                    let mut metadata = base_metadata("service_structure");
                    insert_string(&mut metadata, "port_name", &port_name);
                    if let Some(binding) = xml_element_attribute(element, content, "binding") {
                        insert_string(&mut metadata, "binding", &binding);
                    }
                    facts.push(fact_for_node(
                        file_path,
                        "xml",
                        XML_WSDL_PORT_PATTERN_ID,
                        "port",
                        element,
                        metadata,
                    ));
                }
            }
            "binding" => {
                if let Some(binding_name) = xml_element_attribute(element, content, "name") {
                    let mut metadata = base_metadata("service_structure");
                    insert_string(&mut metadata, "binding_name", &binding_name);
                    if let Some(port_type) = xml_element_attribute(element, content, "type") {
                        insert_string(&mut metadata, "port_type", &port_type);
                    }
                    facts.push(fact_for_node(
                        file_path,
                        "xml",
                        XML_WSDL_BINDING_PATTERN_ID,
                        "binding",
                        element,
                        metadata,
                    ));
                }
            }
            "message" => {
                if let Some(message_name) = xml_element_attribute(element, content, "name") {
                    let mut metadata = base_metadata("service_structure");
                    insert_string(&mut metadata, "message_name", &message_name);
                    metadata.insert(
                        "part_count".to_string(),
                        Value::Number(Number::from(xml_child_element_count(
                            element, content, "part",
                        ))),
                    );
                    facts.push(fact_for_node(
                        file_path,
                        "xml",
                        XML_WSDL_MESSAGE_PATTERN_ID,
                        "message",
                        element,
                        metadata,
                    ));
                }
            }
            "operation" => {
                if let Some(operation_name) = xml_element_attribute(element, content, "name") {
                    let mut metadata = base_metadata("service_structure");
                    insert_string(&mut metadata, "operation_name", &operation_name);
                    if let Some(parent) = xml_parent_element(element) {
                        let parent_local =
                            xml_element_tag_name(parent, content).map(xml_local_name);
                        let parent_kind = match parent_local {
                            Some("portType") => Some("port_type"),
                            Some("binding") => Some("binding"),
                            _ => None,
                        };
                        if let Some(parent_kind) = parent_kind {
                            insert_string(&mut metadata, "parent_kind", parent_kind);
                            if let Some(parent_name) =
                                xml_element_attribute(parent, content, "name")
                            {
                                insert_string(&mut metadata, "parent_name", &parent_name);
                            }
                        }
                    }
                    for (child_local, key) in
                        [("input", "input_message"), ("output", "output_message")]
                    {
                        if let Some(message) =
                            xml_child_element_attribute(element, content, child_local, "message")
                        {
                            insert_string(&mut metadata, key, &message);
                        }
                    }
                    facts.push(fact_for_node(
                        file_path,
                        "xml",
                        XML_WSDL_OPERATION_PATTERN_ID,
                        "operation",
                        element,
                        metadata,
                    ));
                }
            }
            _ => {}
        },
    }
}

fn xml_namespace_declaration_fact(
    attribute: Node<'_>,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let name = child_text(attribute, content, "Name")?;
    let (is_default, prefix) = if name == "xmlns" {
        (true, None)
    } else {
        (false, Some(name.strip_prefix("xmlns:")?))
    };
    let namespace_uri = child_text(attribute, content, "AttValue").map(xml_unquote)?;

    let mut metadata = base_metadata("document_metadata");
    insert_string(&mut metadata, "namespace_uri", namespace_uri);
    metadata.insert("is_default".to_string(), Value::Bool(is_default));
    if let Some(prefix) = prefix {
        insert_string(&mut metadata, "prefix", prefix);
    }

    Some(fact_for_node(
        file_path,
        "xml",
        XML_NAMESPACE_DECLARATION_PATTERN_ID,
        "namespace_declaration",
        attribute,
        metadata,
    ))
}

/// The raw QName an XSD type restricts or extends. Nested `complexType` and
/// `simpleType` declarations own their own derivation, so the search stops at
/// them rather than attributing an inner base to the enclosing type.
fn xsd_base_type(element: Node<'_>, content: &str, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let child_depth = child_tree_depth(depth)?;

    let mut cursor = element.walk();
    for child in element.children(&mut cursor) {
        match child.kind() {
            "content" => {
                if let Some(base) = xsd_base_type(child, content, child_depth) {
                    return Some(base);
                }
            }
            "element" => {
                let local_name = xml_element_tag_name(child, content).map(xml_local_name);
                if matches!(local_name, Some("complexType") | Some("simpleType")) {
                    continue;
                }
                if matches!(local_name, Some("restriction") | Some("extension"))
                    && let Some(base) = xml_element_attribute(child, content, "base")
                {
                    return Some(base);
                }
                if let Some(base) = xsd_base_type(child, content, child_depth) {
                    return Some(base);
                }
            }
            _ => {}
        }
    }

    None
}

fn xml_tag_node<'tree>(element: Node<'tree>) -> Option<Node<'tree>> {
    let mut cursor = element.walk();
    element
        .children(&mut cursor)
        .find(|child| matches!(child.kind(), "STag" | "EmptyElemTag"))
}

fn xml_element_tag_name<'a>(element: Node<'_>, content: &'a str) -> Option<&'a str> {
    child_text(xml_tag_node(element)?, content, "Name")
}

/// `xsd:complexType` and `complexType` name the same component. Prefixes are
/// dropped only to recognise a component; recorded values keep their prefix,
/// because the tier does no namespace resolution.
fn xml_local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn xml_unquote(text: &str) -> &str {
    let unquoted = text.strip_prefix(['"', '\'']).unwrap_or(text);
    unquoted.strip_suffix(['"', '\'']).unwrap_or(unquoted)
}

fn xml_element_attribute(element: Node<'_>, content: &str, attribute: &str) -> Option<String> {
    let tag = xml_tag_node(element)?;
    let mut cursor = tag.walk();
    for child in tag.children(&mut cursor) {
        if child.kind() != "Attribute" {
            continue;
        }
        let Some(name) = child_text(child, content, "Name") else {
            continue;
        };
        if xml_local_name(name) != attribute {
            continue;
        }
        let value = child_text(child, content, "AttValue").map(xml_unquote)?;
        if value.is_empty() {
            return None;
        }
        return Some(value.to_string());
    }
    None
}

fn xml_child_elements<'tree>(element: Node<'tree>) -> Vec<Node<'tree>> {
    let mut children = Vec::new();
    let mut cursor = element.walk();
    for child in element.children(&mut cursor) {
        if child.kind() != "content" {
            continue;
        }
        let mut content_cursor = child.walk();
        for grandchild in child.children(&mut content_cursor) {
            if grandchild.kind() == "element" {
                children.push(grandchild);
            }
        }
    }
    children
}

fn xml_child_element_count(element: Node<'_>, content: &str, local_name: &str) -> u64 {
    xml_child_elements(element)
        .into_iter()
        .filter(|child| {
            xml_element_tag_name(*child, content).map(xml_local_name) == Some(local_name)
        })
        .count() as u64
}

fn xml_child_element_attribute(
    element: Node<'_>,
    content: &str,
    local_name: &str,
    attribute: &str,
) -> Option<String> {
    xml_child_elements(element)
        .into_iter()
        .filter(|child| {
            xml_element_tag_name(*child, content).map(xml_local_name) == Some(local_name)
        })
        .find_map(|child| xml_element_attribute(child, content, attribute))
}

/// The element that encloses `node`, skipping the `content` node the grammar
/// puts between an element and its children.
fn xml_parent_element<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    let mut current = node.parent()?;
    loop {
        match current.kind() {
            "element" => return Some(current),
            "content" => current = current.parent()?,
            _ => return None,
        }
    }
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
    has_child_kind_at_depth(node, child_kind, 0)
}

fn has_child_kind_at_depth(node: Node<'_>, child_kind: &str, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == child_kind || has_child_kind_at_depth(child, child_kind, child_depth) {
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

fn yaml_pair_count(node: Node<'_>) -> usize {
    count_direct_children(node, "block_mapping_pair")
        + count_direct_children(node, "flow_pair")
        + count_direct_children(node, "flow_mapping_pair")
}

fn yaml_sequence_length(node: Node<'_>) -> usize {
    let block_count = count_direct_children(node, "block_sequence_item");
    if block_count > 0 {
        return block_count;
    }
    let mut count = 0usize;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "flow_node" {
            count += 1;
        }
    }
    count
}

fn yaml_node_scalar_text(content: &str, node: Node<'_>) -> Option<String> {
    yaml_node_scalar_text_at_depth(content, node, 0)
}

fn yaml_node_scalar_text_at_depth(content: &str, node: Node<'_>, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if matches!(
        node.kind(),
        "plain_scalar" | "double_quote_scalar" | "single_quote_scalar"
    ) {
        let text = node_text(content, node)?;
        return Some(text.trim_matches('"').trim_matches('\'').trim().to_string());
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(text) = yaml_node_scalar_text_at_depth(content, child, child_depth) {
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
    let mut rendered = "$".to_string();
    for segment in path {
        if segment.starts_with('[') {
            rendered.push_str(segment);
        } else {
            rendered.push('.');
            rendered.push_str(segment);
        }
    }
    rendered
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

fn is_json_value_node_kind(kind: &str) -> bool {
    matches!(
        kind,
        "object" | "array" | "string" | "number" | "true" | "false" | "null"
    )
}

fn count_json_array_elements(node: Node<'_>) -> usize {
    let mut count = 0usize;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_json_value_node_kind(child.kind()) {
            count += 1;
        }
    }
    count
}

fn toml_table_name(content: &str, node: Node<'_>) -> Option<String> {
    toml_table_name_at_depth(content, node, 0)
}

fn toml_table_name_at_depth(content: &str, node: Node<'_>, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "bare_key" | "quoted_key" | "dotted_key" => {
                let name = node_text(content, child)?;
                return Some(name.trim_matches('"').trim_matches('\'').to_string());
            }
            _ => {
                if let Some(name) = toml_table_name_at_depth(content, child, child_depth) {
                    return Some(name);
                }
            }
        }
    }
    None
}

fn toml_pair_key_parts(content: &str, node: Node<'_>) -> Option<Vec<String>> {
    let pair_text = node_text(content, node)?;
    let left = pair_text.split_once('=')?.0.trim();
    parse_toml_key_parts(left)
}

fn parse_toml_key_parts(source: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut part_start = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, ch) {
            (Some(_), '\\') => escaped = true,
            (Some(active), _) if ch == active => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, '.') => {
                push_toml_key_part(source, part_start, index, &mut parts);
                part_start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    push_toml_key_part(source, part_start, source.len(), &mut parts);
    (!parts.is_empty()).then_some(parts)
}

fn push_toml_key_part(source: &str, start: usize, end: usize, parts: &mut Vec<String>) {
    let part = source[start..end]
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    if !part.is_empty() {
        parts.push(part.to_string());
    }
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
    toml_key_path_parts(table_path, &[key.to_string()])
}

fn toml_key_path_parts(table_path: &[String], key_parts: &[String]) -> String {
    let mut path = table_path.to_vec();
    path.extend(key_parts.iter().cloned());
    toml_render_path(&path)
}

fn toml_render_path(path: &[String]) -> String {
    let mut rendered = String::new();
    for segment in path {
        if segment.starts_with('[') {
            rendered.push_str(segment);
        } else {
            if !rendered.is_empty() {
                rendered.push('.');
            }
            rendered.push_str(segment);
        }
    }
    rendered
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

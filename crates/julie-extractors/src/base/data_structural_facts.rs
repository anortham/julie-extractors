use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::attach_containing_symbols;
use super::openapi_route_facts::collect_openapi_route_facts;
use super::span::NormalizedSpan;
use super::structural_fact_builders::{base_metadata, fact_for_node, fact_for_span, insert_string};
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol};
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
const JSON_SCHEMA_DEFINITION_PATTERN_ID: &str = "json.schema_definition.v1";

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
const YAML_REF_PATTERN_ID: &str = "yaml.ref.v1";

// XML
const XML_DOCUMENT_PATTERN_ID: &str = "xml.document.v1";
const XML_NAMESPACE_DECLARATION_PATTERN_ID: &str = "xml.namespace_declaration.v1";
const XML_XSD_TYPE_PATTERN_ID: &str = "xml.xsd.type.v1";
const XML_XSD_ELEMENT_PATTERN_ID: &str = "xml.xsd.element.v1";
const XML_XSD_IMPORT_PATTERN_ID: &str = "xml.xsd.import.v1";
const XML_XSD_SCHEMA_PATTERN_ID: &str = "xml.xsd.schema.v1";
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
const REGEX_INLINE_FLAGS_PATTERN_ID: &str = "regex.inline_flags.v1";
const REGEX_BACKREFERENCE_PATTERN_ID: &str = "regex.backreference.v1";
const REGEX_QUOTED_LITERAL_PATTERN_ID: &str = "regex.quoted_literal.v1";

#[cfg(all(test, feature = "test-capability-matrix"))]
const MARKDOWN_DATA_PATTERN_IDS: &[&str] = &[
    MARKDOWN_FENCED_CODE_BLOCK_PATTERN_ID,
    MARKDOWN_FRONTMATTER_PATTERN_ID,
    MARKDOWN_HEADING_PATTERN_ID,
    MARKDOWN_INLINE_LINK_PATTERN_ID,
    MARKDOWN_LINK_DEFINITION_PATTERN_ID,
    MARKDOWN_TABLE_PATTERN_ID,
    crate::markdown::facts::AUTOLINK_PATTERN_ID,
    crate::markdown::facts::DEFINITION_LIST_ITEM_PATTERN_ID,
    crate::markdown::facts::FOOTNOTE_DEFINITION_PATTERN_ID,
    crate::markdown::facts::FOOTNOTE_REFERENCE_PATTERN_ID,
    crate::markdown::facts::REFERENCE_LINK_PATTERN_ID,
    crate::markdown::facts::TASK_LIST_ITEM_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const JSON_DATA_PATTERN_IDS: &[&str] = &[
    JSON_ARRAY_PATTERN_ID,
    JSON_OBJECT_PATTERN_ID,
    JSON_PROPERTY_PATTERN_ID,
    JSON_REF_PATTERN_ID,
    JSON_SCHEMA_DEFINITION_PATTERN_ID,
    JSON_SCHEMA_PATTERN_ID,
    super::openapi_route_facts::OPENAPI_ROUTE_PATTERN_ID,
    crate::json::manifest::MANIFEST_SCRIPT_PATTERN_ID,
    crate::toml::dependencies::MANIFEST_DEPENDENCY_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const TOML_DATA_PATTERN_IDS: &[&str] = &[
    TOML_ARRAY_TABLE_PATTERN_ID,
    TOML_INLINE_TABLE_PATTERN_ID,
    TOML_KEY_VALUE_PATTERN_ID,
    TOML_TABLE_PATTERN_ID,
    crate::toml::dependencies::MANIFEST_DEPENDENCY_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const YAML_DATA_PATTERN_IDS: &[&str] = &[
    YAML_ALIAS_PATTERN_ID,
    YAML_ANCHOR_PATTERN_ID,
    YAML_DOCUMENT_PATTERN_ID,
    YAML_KEY_VALUE_PATTERN_ID,
    YAML_MAPPING_PATTERN_ID,
    YAML_REF_PATTERN_ID,
    YAML_SEQUENCE_PATTERN_ID,
    super::openapi_route_facts::OPENAPI_ROUTE_PATTERN_ID,
    crate::yaml::ci::CI_JOB_PATTERN_ID,
    crate::yaml::ci::CI_TRIGGER_PATTERN_ID,
    crate::yaml::ci::CI_USES_PATTERN_ID,
    crate::yaml::ansible::ANSIBLE_TASK_PATTERN_ID,
    crate::yaml::compose::COMPOSE_SERVICE_PATTERN_ID,
    crate::yaml::kubernetes::K8S_RESOURCE_PATTERN_ID,
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
    XML_XSD_SCHEMA_PATTERN_ID,
    XML_XSD_TYPE_PATTERN_ID,
    crate::xml::build::MSBUILD_PROPERTY_PATTERN_ID,
    crate::xml::facts::ANDROID_COMPONENT_PATTERN_ID,
    crate::xml::facts::ANDROID_PERMISSION_PATTERN_ID,
    crate::xml::facts::CONFIG_ENTRY_PATTERN_ID,
    crate::xml::facts::DOCUMENT_LINK_PATTERN_ID,
    crate::xml::facts::MYBATIS_STATEMENT_PATTERN_ID,
    crate::xml::facts::SERVLET_ROUTE_PATTERN_ID,
    crate::xml::facts::SPRING_BEAN_PATTERN_ID,
    crate::xml::facts::SPRING_COMPONENT_SCAN_PATTERN_ID,
    crate::xml::facts::TEST_SELECTION_PATTERN_ID,
    crate::toml::dependencies::MANIFEST_DEPENDENCY_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const REGEX_DATA_PATTERN_IDS: &[&str] = &[
    REGEX_ALTERNATION_PATTERN_ID,
    REGEX_ANCHOR_PATTERN_ID,
    REGEX_BACKREFERENCE_PATTERN_ID,
    REGEX_CAPTURE_GROUP_PATTERN_ID,
    REGEX_CHARACTER_CLASS_PATTERN_ID,
    REGEX_INLINE_FLAGS_PATTERN_ID,
    REGEX_LOOKAROUND_PATTERN_ID,
    REGEX_NAMED_CAPTURE_PATTERN_ID,
    REGEX_QUANTIFIER_PATTERN_ID,
    REGEX_QUOTED_LITERAL_PATTERN_ID,
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
        "regex" => collect_regex_structural_facts(file_path, content),
        _ => Vec::new(),
    };
    if language == "markdown" {
        facts.extend(crate::markdown::facts::markdown_facts(
            tree, file_path, content, symbols,
        ));
    }
    if language == "xml" {
        facts.extend(crate::xml::build::build_facts(tree, file_path, content));
        facts.extend(crate::xml::facts::xml_facts(tree, file_path, content));
    }
    if language == "yaml" {
        facts.extend(crate::yaml::ci::ci_facts(tree, file_path, content, symbols));
        facts.extend(crate::yaml::domain_facts(tree, file_path, content));
    }
    if language == "json" {
        facts.extend(crate::json::manifest::manifest_facts(
            tree.root_node(),
            file_path,
            content,
        ));
    }
    if language == "toml" {
        facts.extend(crate::toml::dependencies::dependency_facts(
            tree.root_node(),
            file_path,
            content,
        ));
    }
    if language == "erlang" {
        facts.extend(crate::erlang::term_config::dependency_facts(
            tree, file_path, content,
        ));
    }
    if matches!(language, "json" | "yaml") {
        facts.extend(collect_openapi_route_facts(
            language, tree, file_path, content, symbols,
        ));
    }

    if language == "regex" {
        crate::regex::attach_fact_symbols(&mut facts, symbols);
    } else if language == "json" {
        super::containing_symbol::attach_byte_containing_symbols(&mut facts, symbols);
    } else if language == "xml" {
        super::containing_symbol::attach_declaring_symbols(&mut facts, symbols);
    } else {
        attach_containing_symbols(&mut facts, symbols);
    }
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
        "erlang" => &[crate::toml::dependencies::MANIFEST_DEPENDENCY_PATTERN_ID],
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
    for inline_tree in crate::markdown::inline::parse_inline_trees(tree, content) {
        collect_markdown_node(inline_tree.root_node(), file_path, content, &mut facts, 0);
        for link in crate::markdown::inline::nested_bracket_links(&inline_tree, content) {
            let Some(span) = NormalizedSpan::from_content_range(content, link.start, link.end)
            else {
                continue;
            };
            let mut metadata = base_metadata("document_links");
            insert_string(&mut metadata, "label", &link.label);
            insert_string(&mut metadata, "destination", &link.destination);
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
    facts
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
        "atx_heading" | "setext_heading" | "heading" => {
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
    let keys: Vec<Value> = crate::markdown::blocks::frontmatter_keys(content, node)
        .into_iter()
        .map(|key| Value::String(key.name))
        .collect();

    let mut metadata = base_metadata("document_metadata");
    insert_string(&mut metadata, "format", format);
    metadata.insert(
        "key_count".to_string(),
        Value::Number(Number::from(key_count)),
    );
    if !keys.is_empty() {
        metadata.insert("keys".to_string(), Value::Array(keys));
    }

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
    let level = crate::markdown::setext_level(node)
        .unwrap_or_else(|| text.chars().take_while(|ch| *ch == '#').count().clamp(1, 6));
    let (heading_text, anchor) = crate::markdown::blocks::heading_name(content, node)?;

    let mut metadata = base_metadata("document_structure");
    metadata.insert("level".to_string(), Value::Number(Number::from(level)));
    insert_string(&mut metadata, "text", &heading_text);
    if let Some(anchor) = anchor {
        insert_string(&mut metadata, "anchor", &anchor);
    }

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
    let mut metadata = base_metadata("document_structure");
    if let Some(language) = crate::markdown::blocks::fence_language(content, node) {
        insert_string(&mut metadata, "language", &language);
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
    let label = crate::markdown::inline::plain_text(content, child_node(node, "link_text")?);
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
    if label.starts_with('^') {
        return None;
    }
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
    let declares_schema = facts
        .iter()
        .any(|fact| fact.pattern_id == JSON_SCHEMA_PATTERN_ID);
    collect_json_schema_definitions(
        tree.root_node(),
        file_path,
        content,
        &[],
        declares_schema,
        &mut facts,
        0,
    );
    facts
}

/// One `json.schema_definition.v1` fact per object entry under `$defs` (at any
/// depth), `definitions` (at the root, or at any depth in a document that
/// declares `$schema`), and the root `components.schemas` of OpenAPI.
fn collect_json_schema_definitions(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    path: &[String],
    declares_schema: bool,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let container = match path {
        [.., last] if last == "$defs" => Some("$defs"),
        [only] if only == "definitions" => Some("definitions"),
        [.., last] if last == "definitions" && declares_schema => Some("definitions"),
        [first, second] if first == "components" && second == "schemas" => {
            Some("components.schemas")
        }
        _ => None,
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let (child_path, value) = match child.kind() {
            "pair" => {
                let (Some(key), Some(value)) =
                    (json_pair_key(content, child), json_pair_value(child))
                else {
                    continue;
                };
                if let Some(container) = container
                    && value.kind() == "object"
                {
                    facts.push(json_schema_definition_fact(
                        file_path, content, child, &key, container, path, value,
                    ));
                }
                let mut child_path = path.to_vec();
                child_path.push(key);
                (child_path, value)
            }
            "object" | "document" => (path.to_vec(), child),
            "array" => {
                let mut element_cursor = child.walk();
                for (index, element) in child
                    .named_children(&mut element_cursor)
                    .filter(|element| is_json_value_node_kind(element.kind()))
                    .enumerate()
                {
                    let mut element_path = path.to_vec();
                    element_path.push(format!("[{index}]"));
                    collect_json_schema_definitions(
                        element,
                        file_path,
                        content,
                        &element_path,
                        declares_schema,
                        facts,
                        child_depth,
                    );
                }
                continue;
            }
            _ => continue,
        };
        collect_json_schema_definitions(
            value,
            file_path,
            content,
            &child_path,
            declares_schema,
            facts,
            child_depth,
        );
    }
}

#[inline(never)]
fn json_schema_definition_fact(
    file_path: &str,
    content: &str,
    pair: Node<'_>,
    name: &str,
    container: &str,
    container_path: &[String],
    definition: Node<'_>,
) -> StructuralFact {
    let mut metadata = base_metadata("schema_structure");
    insert_string(&mut metadata, "name", name);
    insert_string(&mut metadata, "container", container);
    insert_string(&mut metadata, "path", &json_path(container_path));
    let mut cursor = definition.walk();
    for field in definition
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "pair")
    {
        let (Some(key), Some(value)) = (json_pair_key(content, field), json_pair_value(field))
        else {
            continue;
        };
        match key.as_str() {
            "type" => {
                let declared = match value.kind() {
                    "array" => {
                        let mut type_cursor = value.walk();
                        value
                            .named_children(&mut type_cursor)
                            .filter_map(|item| json_string_value(content, item))
                            .collect::<Vec<_>>()
                            .join("|")
                    }
                    _ => json_string_value(content, value).unwrap_or_default(),
                };
                if !declared.is_empty() {
                    insert_string(&mut metadata, "declared_type", &declared);
                }
            }
            "allOf" | "oneOf" | "anyOf" if !metadata.contains_key("composition") => {
                insert_string(&mut metadata, "composition", &key);
            }
            _ => {}
        }
    }
    fact_for_node(
        file_path,
        "json",
        JSON_SCHEMA_DEFINITION_PATTERN_ID,
        "definition",
        pair,
        metadata,
    )
}

fn json_string_value(content: &str, node: Node<'_>) -> Option<String> {
    (node.kind() == "string")
        .then(|| node_text(content, node))
        .flatten()
        .map(crate::json::decode_json_string)
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
    let table_paths = toml_table_paths(tree.root_node(), content);
    collect_toml_node(
        tree.root_node(),
        file_path,
        content,
        &[],
        &table_paths,
        &mut facts,
        0,
    );
    facts
}

/// The key path of each table header, by start byte. An array-of-tables
/// element and every table under it carry the element index:
/// `[[products]]` twice, then `[products.dims]` -> `products[1].dims`.
fn toml_table_paths(
    root: Node<'_>,
    content: &str,
) -> std::collections::HashMap<usize, Vec<String>> {
    let mut array_index: std::collections::HashMap<Vec<String>, usize> =
        std::collections::HashMap::new();
    let mut paths = std::collections::HashMap::new();
    let mut cursor = root.walk();
    for table in root
        .named_children(&mut cursor)
        .filter(|node| matches!(node.kind(), "table" | "table_array_element"))
    {
        let Some(parts) = crate::toml::dependencies::header_parts(table, content) else {
            continue;
        };
        if table.kind() == "table_array_element" {
            array_index
                .retain(|header, _| !(header.len() > parts.len() && header.starts_with(&parts)));
            let next = array_index.get(&parts).map_or(0, |index| index + 1);
            array_index.insert(parts.clone(), next);
        }
        let mut path = Vec::new();
        for end in 1..=parts.len() {
            path.push(parts[end - 1].clone());
            if let Some(index) = array_index.get(&parts[..end]) {
                path.push(format!("[{index}]"));
            }
        }
        paths.insert(table.start_byte(), path);
    }
    paths
}

fn collect_toml_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    table_path: &[String],
    table_paths: &std::collections::HashMap<usize, Vec<String>>,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "table" => {
            if let Some(table_name) = crate::toml::header_name(node, content) {
                let own_path = table_paths
                    .get(&node.start_byte())
                    .cloned()
                    .unwrap_or_else(|| vec![table_name.clone()]);
                let mut metadata = base_metadata("config_structure");
                insert_string(&mut metadata, "table_name", &table_name);
                insert_string(
                    &mut metadata,
                    "key_path",
                    &toml_key_path_parts(table_path, &own_path),
                );
                metadata.insert("is_array_table".to_string(), Value::Bool(false));
                if let Some(span) = NormalizedSpan::from_content_range_with_line_starts(
                    content,
                    &[],
                    node.start_byte(),
                    crate::toml::table_end_byte(node),
                ) {
                    facts.push(fact_for_span(
                        file_path,
                        "toml",
                        TOML_TABLE_PATTERN_ID,
                        "table",
                        node.kind(),
                        span,
                        metadata,
                    ));
                }

                let mut child_path = table_path.to_vec();
                child_path.extend(own_path);
                walk_toml_children(
                    node,
                    file_path,
                    content,
                    &child_path,
                    table_paths,
                    facts,
                    depth,
                );
                return;
            }
        }
        "table_array_element" => {
            if let Some(table_name) = crate::toml::header_name(node, content) {
                let own_path = table_paths
                    .get(&node.start_byte())
                    .cloned()
                    .unwrap_or_else(|| vec![table_name.clone()]);
                let mut metadata = base_metadata("config_structure");
                insert_string(&mut metadata, "table_name", &table_name);
                insert_string(
                    &mut metadata,
                    "key_path",
                    &toml_key_path_parts(table_path, &own_path),
                );
                metadata.insert("is_array_table".to_string(), Value::Bool(true));
                if let Some(span) = NormalizedSpan::from_content_range_with_line_starts(
                    content,
                    &[],
                    node.start_byte(),
                    crate::toml::table_end_byte(node),
                ) {
                    facts.push(fact_for_span(
                        file_path,
                        "toml",
                        TOML_ARRAY_TABLE_PATTERN_ID,
                        "array_table",
                        node.kind(),
                        span,
                        metadata,
                    ));
                }

                let mut child_path = table_path.to_vec();
                child_path.extend(own_path);
                walk_toml_children(
                    node,
                    file_path,
                    content,
                    &child_path,
                    table_paths,
                    facts,
                    depth,
                );
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
                                table_paths,
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
                            table_paths,
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
                walk_toml_children(
                    node,
                    file_path,
                    content,
                    &inline_path,
                    table_paths,
                    facts,
                    depth,
                );
                return;
            }
        }
        "array" => {
            collect_toml_array_children(
                node,
                file_path,
                content,
                table_path,
                table_paths,
                facts,
                depth,
            );
            return;
        }
        _ => {}
    }

    walk_toml_children(
        node,
        file_path,
        content,
        table_path,
        table_paths,
        facts,
        depth,
    );
}

fn walk_toml_children(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    table_path: &[String],
    table_paths: &std::collections::HashMap<usize, Vec<String>>,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_toml_node(
            child,
            file_path,
            content,
            table_path,
            table_paths,
            facts,
            child_depth,
        );
    }
}

fn collect_toml_array_children(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    table_path: &[String],
    table_paths: &std::collections::HashMap<usize, Vec<String>>,
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
        walk_toml_children(
            child,
            file_path,
            content,
            &indexed_path,
            table_paths,
            facts,
            child_depth,
        );
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
    if value_node.kind() == "string"
        && let Some((_, style)) =
            node_text(content, value_node).and_then(crate::toml::text::decode_toml_string)
    {
        insert_string(&mut metadata, "string_style", style);
    }
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
    let root = tree.root_node();
    let mut cursor = root.walk();
    let documents: Vec<Node<'_>> = root
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "document")
        .collect();
    let multi = documents.len() > 1;
    for (index, document) in documents.into_iter().enumerate() {
        let doc = YamlDocument { index, multi };
        collect_yaml_node(document, file_path, content, &[], doc, &mut facts, 0);
    }
    facts
}

/// The position of a document in a YAML stream. Facts in a stream of more
/// than one document carry `document_index`, so `(document_index, key_path)`
/// names one location.
#[derive(Clone, Copy)]
struct YamlDocument {
    index: usize,
    multi: bool,
}

impl YamlDocument {
    fn tag(self, metadata: &mut std::collections::HashMap<String, Value>) {
        if self.multi {
            metadata.insert(
                "document_index".to_string(),
                Value::Number(Number::from(self.index)),
            );
        }
    }
}

fn collect_yaml_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    path: &[String],
    doc: YamlDocument,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    match node.kind() {
        "document" => {
            let mut metadata = base_metadata("config_structure");
            metadata.insert(
                "has_directives".to_string(),
                Value::Bool(has_child_kind(node, "directive")),
            );
            metadata.insert(
                "document_index".to_string(),
                Value::Number(Number::from(doc.index)),
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
            insert_string(&mut metadata, "key_path", &json_path(path));
            metadata.insert(
                "pair_count".to_string(),
                Value::Number(Number::from(yaml_pair_count(node))),
            );
            doc.tag(&mut metadata);
            facts.push(fact_for_node(
                file_path,
                "yaml",
                YAML_MAPPING_PATTERN_ID,
                "mapping",
                node,
                metadata,
            ));
        }
        "block_mapping_pair" | "flow_pair" => {
            if let (Some(key), Some(value_node)) = (
                crate::yaml::mapping_key(content, node),
                node.child_by_field_name("value"),
            ) {
                facts.push(yaml_key_value_fact(
                    file_path, content, node, path, &key, value_node, doc,
                ));
                if key == "$ref"
                    && let Some(target) = yaml_node_scalar_text(content, value_node)
                {
                    let mut metadata = base_metadata("schema_structure");
                    insert_string(&mut metadata, "ref", &target);
                    insert_string(&mut metadata, "key_path", &json_path(path));
                    doc.tag(&mut metadata);
                    facts.push(fact_for_node(
                        file_path,
                        "yaml",
                        YAML_REF_PATTERN_ID,
                        "ref",
                        node,
                        metadata,
                    ));
                }

                let mut child_path = path.to_vec();
                child_path.push(key);
                collect_yaml_node(
                    value_node,
                    file_path,
                    content,
                    &child_path,
                    doc,
                    facts,
                    child_depth,
                );
                return;
            }
        }
        "block_sequence" | "flow_sequence" => {
            let mut metadata = base_metadata("config_structure");
            insert_string(&mut metadata, "key_path", &json_path(path));
            metadata.insert(
                "sequence_length".to_string(),
                Value::Number(Number::from(yaml_sequence_length(node))),
            );
            doc.tag(&mut metadata);
            facts.push(fact_for_node(
                file_path,
                "yaml",
                YAML_SEQUENCE_PATTERN_ID,
                "sequence",
                node,
                metadata,
            ));
            let mut cursor = node.walk();
            let mut index = 0usize;
            for child in node.children(&mut cursor) {
                if !matches!(
                    child.kind(),
                    "block_sequence_item" | "flow_node" | "flow_pair"
                ) {
                    collect_yaml_node(child, file_path, content, path, doc, facts, child_depth);
                    continue;
                }
                let mut item_path = path.to_vec();
                item_path.push(format!("[{index}]"));
                collect_yaml_node(
                    child,
                    file_path,
                    content,
                    &item_path,
                    doc,
                    facts,
                    child_depth,
                );
                index += 1;
            }
            return;
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
        collect_yaml_node(child, file_path, content, path, doc, facts, child_depth);
    }
}

#[inline(never)]
fn yaml_key_value_fact(
    file_path: &str,
    content: &str,
    pair: Node<'_>,
    path: &[String],
    key: &str,
    value: Node<'_>,
    doc: YamlDocument,
) -> StructuralFact {
    let mut property_path = path.to_vec();
    property_path.push(key.to_string());
    let mut metadata = base_metadata("config_structure");
    insert_string(&mut metadata, "key", key);
    insert_string(&mut metadata, "key_path", &json_path(&property_path));
    let mut cursor = value.walk();
    let children: Vec<Node<'_>> = value.named_children(&mut cursor).collect();
    let mut value_kind = "other";
    for child in &children {
        match child.kind() {
            "anchor" => {
                if let Some(name) = first_child_text(*child, content, "anchor_name") {
                    insert_string(&mut metadata, "anchor", name.trim());
                }
            }
            "tag" => {
                if let Some(tag) = node_text(content, *child) {
                    insert_string(&mut metadata, "tag", tag.trim());
                }
            }
            kind => {
                value_kind = yaml_content_kind(kind);
                if let Some(style) = yaml_scalar_style(kind, node_text(content, *child)) {
                    insert_string(&mut metadata, "scalar_style", style.0);
                    if let Some(chomping) = style.1 {
                        insert_string(&mut metadata, "chomping", chomping);
                    }
                }
            }
        }
    }
    insert_string(&mut metadata, "value_kind", value_kind);
    doc.tag(&mut metadata);
    fact_for_node(
        file_path,
        "yaml",
        YAML_KEY_VALUE_PATTERN_ID,
        "key_value",
        pair,
        metadata,
    )
}

fn yaml_content_kind(kind: &str) -> &'static str {
    match kind {
        "block_mapping" | "flow_mapping" => "mapping",
        "block_sequence" | "flow_sequence" => "sequence",
        "plain_scalar" | "double_quote_scalar" | "single_quote_scalar" => "scalar",
        "block_scalar" => "block_scalar",
        "alias" => "alias",
        _ => "other",
    }
}

/// `(scalar_style, chomping)` of a scalar value node. Block scalars read the
/// indicator after `|` or `>`: `-` strips, `+` keeps, neither clips.
fn yaml_scalar_style(
    kind: &str,
    text: Option<&str>,
) -> Option<(&'static str, Option<&'static str>)> {
    match kind {
        "plain_scalar" => Some(("plain", None)),
        "single_quote_scalar" => Some(("single_quoted", None)),
        "double_quote_scalar" => Some(("double_quoted", None)),
        "block_scalar" => {
            let header = text?.lines().next()?.trim();
            let style = if header.starts_with('>') {
                "folded"
            } else {
                "literal"
            };
            let chomping = if header.contains('-') {
                "strip"
            } else if header.contains('+') {
                "keep"
            } else {
                "clip"
            };
            Some((style, Some(chomping)))
        }
        _ => None,
    }
}

fn collect_regex_structural_facts(file_path: &str, content: &str) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for pattern_tree in crate::regex::pattern_trees(content) {
        let captures = crate::regex::CaptureInventory::of(pattern_tree.root_node(), content);
        let mut capture_index = 0usize;
        collect_regex_node(
            pattern_tree.root_node(),
            &RegexFactContext {
                file_path,
                content,
                captures: &captures,
            },
            &mut facts,
            &mut capture_index,
            0,
        );
    }
    facts
}

struct RegexFactContext<'a> {
    file_path: &'a str,
    content: &'a str,
    captures: &'a crate::regex::CaptureInventory,
}

fn collect_regex_node(
    node: Node<'_>,
    context: &RegexFactContext<'_>,
    facts: &mut Vec<StructuralFact>,
    capture_index: &mut usize,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let file_path = context.file_path;
    let content = context.content;
    let fact = match node.kind() {
        "named_capturing_group" => {
            *capture_index += 1;
            regex_named_capture_fact(file_path, content, node, *capture_index)
        }
        "anonymous_capturing_group" => {
            *capture_index += 1;
            regex_capture_group_fact(file_path, content, node, *capture_index)
        }
        "lookaround_assertion" => regex_lookaround_fact(file_path, content, node),
        "character_class" => regex_character_class_fact(file_path, content, node),
        kind if crate::regex::is_quantifier_kind(kind) => {
            regex_quantifier_fact(file_path, content, node)
        }
        "alternation" => regex_alternation_fact(file_path, node),
        "start_assertion"
        | "end_assertion"
        | "boundary_assertion"
        | "non_boundary_assertion"
        | "identity_escape" => regex_anchor_fact(file_path, content, node),
        "inline_flags_group" => regex_inline_flags_fact(file_path, content, node),
        "decimal_escape" | "backreference_escape" | "named_group_backreference" => {
            regex_backreference_fact(context, node)
        }
        "term" => {
            facts.extend(regex_quoted_literal_facts(file_path, content, node));
            None
        }
        _ => None,
    };
    facts.extend(fact);

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_regex_node(child, context, facts, capture_index, child_depth);
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
    let direction = crate::regex::flags::get_lookaround_direction(text);
    let polarity = if crate::regex::flags::is_positive_lookaround(text) {
        "positive"
    } else {
        "negative"
    };

    let mut metadata = base_metadata("pattern_structure");
    insert_string(&mut metadata, "direction", &direction);
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

/// The pinned grammar has no possessive quantifier: `a++` parses as `+` then an
/// ERROR node holding the second `+`, which marks the quantifier possessive.
fn regex_quantifier_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let text = node_text(content, node)?.trim();
    let possessive_marker = node
        .next_sibling()
        .filter(|next| next.kind() == "ERROR" && node_text(content, *next) == Some("+"));
    let mut metadata = base_metadata("pattern_structure");
    let Some(marker) = possessive_marker else {
        insert_string(&mut metadata, "quantifier", text);
        return Some(fact_for_node(
            file_path,
            "regex",
            REGEX_QUANTIFIER_PATTERN_ID,
            "quantifier",
            node,
            metadata,
        ));
    };
    insert_string(&mut metadata, "quantifier", &format!("{text}+"));
    metadata.insert("possessive".to_string(), Value::Bool(true));
    let span = NormalizedSpan::from_content_range(content, node.start_byte(), marker.end_byte())?;
    Some(fact_for_span(
        file_path,
        "regex",
        REGEX_QUANTIFIER_PATTERN_ID,
        "quantifier",
        node.kind(),
        span,
        metadata,
    ))
}

fn regex_alternation_fact(file_path: &str, node: Node<'_>) -> Option<StructuralFact> {
    let mut cursor = node.walk();
    let separators = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "|")
        .count();
    let mut metadata = base_metadata("pattern_structure");
    metadata.insert(
        "branch_count".to_string(),
        Value::Number(Number::from(separators + 1)),
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

/// `\A`, `\z`, `\Z` and `\G` parse as identity escapes in the pinned grammar;
/// any other identity escape is a literal character, not an anchor.
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
        r"\G" => "previous_match_end",
        _ if node.kind() == "identity_escape" => return None,
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

fn regex_inline_flags_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let mut cursor = node.walk();
    let flags_text = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "flags")
        .and_then(|flags| node_text(content, flags))
        .unwrap_or_default();
    let header = node_text(content, node)?
        .strip_prefix("(?")?
        .split([':', ')'])
        .next()
        .unwrap_or(flags_text);
    let (enabled, disabled) = header.split_once('-').unwrap_or((header, ""));
    let mut metadata = base_metadata("pattern_structure");
    insert_string(&mut metadata, "enabled_flags", enabled);
    insert_string(&mut metadata, "disabled_flags", disabled);
    metadata.insert(
        "scoped".to_string(),
        Value::Bool(crate::regex::complexity_metrics::is_scoped_inline_flags_group(node)),
    );

    Some(fact_for_node(
        file_path,
        "regex",
        REGEX_INLINE_FLAGS_PATTERN_ID,
        "inline_flags",
        node,
        metadata,
    ))
}

fn regex_backreference_fact(
    context: &RegexFactContext<'_>,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(context.content, node)?;
    let mut metadata = base_metadata("pattern_structure");
    let resolved = match node.kind() {
        "decimal_escape" => {
            let index: usize = text.strip_prefix('\\')?.parse().ok()?;
            insert_string(&mut metadata, "form", "numeric");
            metadata.insert("capture_index".to_string(), Value::Number(index.into()));
            (1..=context.captures.count).contains(&index)
        }
        kind => {
            let name = crate::regex::groups::group_name_node(node)
                .and_then(|name| node_text(context.content, name))
                .or_else(|| {
                    text.strip_prefix("\\k<")
                        .and_then(|rest| rest.strip_suffix('>'))
                })?;
            let form = if kind == "named_group_backreference" {
                "python_named"
            } else {
                "named"
            };
            insert_string(&mut metadata, "form", form);
            insert_string(&mut metadata, "capture_name", name);
            context.captures.names.contains(name)
        }
    };
    metadata.insert("resolved".to_string(), Value::Bool(resolved));

    Some(fact_for_node(
        context.file_path,
        "regex",
        REGEX_BACKREFERENCE_PATTERN_ID,
        "backreference",
        node,
        metadata,
    ))
}

/// A `\Q...\E` quoted span parses as sibling escapes and characters in one term;
/// an unclosed `\Q` quotes to the end of the term.
fn regex_quoted_literal_facts(
    file_path: &str,
    content: &str,
    term: Node<'_>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut cursor = term.walk();
    let children: Vec<Node<'_>> = term.children(&mut cursor).collect();
    let mut index = 0;
    while index < children.len() {
        let start = children[index];
        if !(start.kind() == "identity_escape" && node_text(content, start) == Some(r"\Q")) {
            index += 1;
            continue;
        }
        let close = children[index + 1..]
            .iter()
            .position(|child| {
                child.kind() == "identity_escape" && node_text(content, *child) == Some(r"\E")
            })
            .map(|offset| index + 1 + offset);
        let last = close.map_or(children.len() - 1, |close| close);
        let text_end = close.map_or(children[last].end_byte(), |close| {
            children[close].start_byte()
        });
        let literal = content.get(start.end_byte()..text_end).unwrap_or_default();
        if let Some(span) = NormalizedSpan::from_content_range(
            content,
            start.start_byte(),
            children[last].end_byte(),
        ) {
            let mut metadata = base_metadata("pattern_structure");
            insert_string(&mut metadata, "literal_text", literal);
            metadata.insert("closed".to_string(), Value::Bool(close.is_some()));
            facts.push(fact_for_span(
                file_path,
                "regex",
                REGEX_QUOTED_LITERAL_PATTERN_ID,
                "quoted_literal",
                "quoted_literal",
                span,
                metadata,
            ));
        }
        index = last + 1;
    }
    facts
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
    target_namespace: Option<String>,
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
        if let Some(target_namespace) = &stats.target_namespace {
            insert_string(&mut metadata, "target_namespace", target_namespace);
        }
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
                stats.target_namespace =
                    xml_element_attribute(node, document.content, "targetNamespace");
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

    let in_inline_schema = dialect == XmlDialect::Service
        && std::iter::successors(Some(element), |node| xml_parent_element(*node))
            .any(|node| xml_element_tag_name(node, content).map(xml_local_name) == Some("schema"));
    let dialect = if in_inline_schema {
        XmlDialect::Schema
    } else {
        dialect
    };
    match dialect {
        XmlDialect::Document => {}
        XmlDialect::Schema => match local_name {
            "schema" => {
                let mut metadata = base_metadata("schema_structure");
                for (key, attribute) in [
                    ("target_namespace", "targetNamespace"),
                    ("element_form_default", "elementFormDefault"),
                    ("attribute_form_default", "attributeFormDefault"),
                    ("version", "version"),
                ] {
                    if let Some(value) = xml_element_attribute(element, content, attribute) {
                        insert_string(&mut metadata, key, &value);
                    }
                }
                facts.push(fact_for_node(
                    file_path,
                    "xml",
                    XML_XSD_SCHEMA_PATTERN_ID,
                    "schema",
                    element,
                    metadata,
                ));
            }
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
            "port" | "endpoint" => {
                if let Some(port_name) = xml_element_attribute(element, content, "name") {
                    let mut metadata = base_metadata("service_structure");
                    insert_string(&mut metadata, "port_name", &port_name);
                    if let Some(binding) = xml_element_attribute(element, content, "binding") {
                        insert_string(&mut metadata, "binding", &binding);
                    }
                    let address =
                        xml_element_attribute(element, content, "address").or_else(|| {
                            xml_child_elements(element)
                                .into_iter()
                                .filter(|child| {
                                    xml_element_tag_name(*child, content).map(xml_local_name)
                                        == Some("address")
                                })
                                .find_map(|child| xml_element_attribute(child, content, "location"))
                        });
                    if let Some(address) = address {
                        insert_string(&mut metadata, "address_location", &address);
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

fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

fn child_node<'tree>(node: Node<'tree>, child_kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == child_kind)
}

fn child_text<'a>(node: Node<'_>, content: &'a str, child_kind: &str) -> Option<&'a str> {
    node_text(content, child_node(node, child_kind)?)
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

fn clean_markdown_link_destination(raw: &str) -> String {
    let raw = raw.trim();
    raw.strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
        .unwrap_or(raw)
        .trim()
        .to_string()
}

fn clean_markdown_link_title(raw: &str) -> String {
    raw.trim_matches(|ch| ch == '"' || ch == '\'' || ch == '(' || ch == ')')
        .trim()
        .to_string()
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

fn parse_link_reference_definition(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let (label_part, rest) = trimmed.split_once("]:")?;
    let label = label_part.trim_start_matches('[').trim().to_string();
    let destination = rest.split_whitespace().next()?.to_string();
    (!label.is_empty() && !destination.is_empty()).then_some((label, destination))
}

/// JSONPath of a key chain: `$.a.b[0]`. A key that is not a plain name is
/// bracket-quoted (`$['files.exclude']`), so each path names one location.
fn json_path(path: &[String]) -> String {
    let mut rendered = "$".to_string();
    for segment in path {
        if is_json_index_segment(segment) {
            rendered.push_str(segment);
        } else if is_plain_json_path_name(segment) {
            rendered.push('.');
            rendered.push_str(segment);
        } else {
            rendered.push_str("['");
            rendered.push_str(&segment.replace('\\', "\\\\").replace('\'', "\\'"));
            rendered.push_str("']");
        }
    }
    rendered
}

fn is_json_index_segment(segment: &str) -> bool {
    segment
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .is_some_and(|index| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
}

fn is_plain_json_path_name(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|ch| !matches!(ch, '.' | '[' | ']' | '\'' | '"' | '\\') && !ch.is_whitespace())
}

fn json_pair_key(content: &str, node: Node<'_>) -> Option<String> {
    let key_node = node.child(0)?;
    let text = node_text(content, key_node)?;
    Some(crate::json::decode_json_string(text))
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

fn toml_pair_key_parts(content: &str, node: Node<'_>) -> Option<Vec<String>> {
    crate::toml::dependencies::pair_key_parts(node, content)
}

fn toml_pair_value(node: Node<'_>) -> Option<Node<'_>> {
    crate::toml::pair_value(node)
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

use super::parsing::{VueSection, parse_vue_sfc};
use crate::base::relationship_resolution::{StructuredPendingRelationship, UnresolvedTarget};
use crate::base::{BaseExtractor, Relationship, RelationshipKind, Symbol, SymbolKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use tree_sitter::{Node, Parser};

static TEMPLATE_INTERPOLATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{\s*([^}]+?)\s*\}\}").unwrap());
static TEMPLATE_EVENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"@[A-Za-z0-9:_-]+\s*=\s*"([^"]+)""#).unwrap());
static COMPONENT_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([A-Za-z][A-Za-z0-9_-]*)\b").unwrap());
static IDENTIFIER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z_$][A-Za-z0-9_$]*").unwrap());

pub(super) fn extract_relationships(base: &BaseExtractor, symbols: &[Symbol]) -> Vec<Relationship> {
    let Ok(sections) = parse_vue_sfc(&base.content) else {
        return Vec::new();
    };

    let local_symbols = unique_symbols_by_name(symbols);
    let Some(component) = component_symbol(symbols) else {
        return Vec::new();
    };

    let mut relationships = Vec::new();
    let mut seen = HashSet::new();

    for section in &sections {
        match section.section_type.as_str() {
            "script" => collect_script_relationships(
                base,
                section,
                component,
                &local_symbols,
                &mut relationships,
                &mut seen,
            ),
            "template" => collect_template_relationships(
                base,
                section,
                component,
                &local_symbols,
                &mut relationships,
                &mut seen,
            ),
            _ => {}
        }
    }

    relationships
}

pub(super) fn extract_structured_pending_relationships(
    base: &BaseExtractor,
    symbols: &[Symbol],
) -> Vec<StructuredPendingRelationship> {
    let Ok(sections) = parse_vue_sfc(&base.content) else {
        return Vec::new();
    };
    let Some(component) = component_symbol(symbols) else {
        return Vec::new();
    };
    let local_symbols = unique_local_callables_by_name(symbols);
    let imported_modules = import_sources_by_name(symbols);
    let mut pending = Vec::new();
    let mut seen_template = HashSet::new();
    let mut seen_script = HashSet::new();

    for section in &sections {
        match section.section_type.as_str() {
            "template" => {
                for (line_index, line) in section.content.lines().enumerate() {
                    let line_number = section.start_line as u32 + line_index as u32 + 1;
                    for captures in COMPONENT_TAG_RE.captures_iter(line) {
                        let Some(tag_name) = captures.get(1).map(|matched| matched.as_str()) else {
                            continue;
                        };
                        if !is_component_tag(tag_name) || local_symbols.contains_key(tag_name) {
                            continue;
                        }
                        if !seen_template.insert((tag_name.to_string(), line_number)) {
                            continue;
                        }
                        pending.push(StructuredPendingRelationship::new(
                            component.id.clone(),
                            UnresolvedTarget::simple(tag_name),
                            Some(component.id.clone()),
                            RelationshipKind::References,
                            base.file_path.clone(),
                            line_number,
                            1.0,
                        ));
                    }
                }
            }
            "script" => {
                let Some(tree) = parse_script_section(section) else {
                    continue;
                };
                visit_script_pending_node(
                    base,
                    tree.root_node(),
                    &section.content,
                    section.start_line,
                    component,
                    &local_symbols,
                    &imported_modules,
                    &mut pending,
                    &mut seen_script,
                    0,
                );
            }
            _ => {}
        }
    }

    pending
}

fn unique_local_callables_by_name(symbols: &[Symbol]) -> HashMap<String, &Symbol> {
    let mut grouped: HashMap<&str, Vec<&Symbol>> = HashMap::new();
    for symbol in symbols {
        if symbol.kind == SymbolKind::Import {
            continue;
        }
        grouped
            .entry(symbol.name.as_str())
            .or_default()
            .push(symbol);
    }
    grouped
        .into_iter()
        .filter_map(|(name, symbols)| {
            if symbols.len() == 1 {
                Some((name.to_string(), symbols[0]))
            } else {
                None
            }
        })
        .collect()
}

fn import_sources_by_name(symbols: &[Symbol]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for symbol in symbols {
        if symbol.kind != SymbolKind::Import {
            continue;
        }
        let Some(metadata) = symbol.metadata.as_ref() else {
            continue;
        };
        let Some(source) = metadata.get("source").and_then(Value::as_str) else {
            continue;
        };
        map.insert(symbol.name.clone(), source.to_string());
    }
    map
}

#[allow(clippy::too_many_arguments)]
fn visit_script_pending_node(
    base: &BaseExtractor,
    node: Node,
    script_content: &str,
    start_line_offset: usize,
    component: &Symbol,
    local_symbols: &HashMap<String, &Symbol>,
    imports: &HashMap<String, String>,
    pending: &mut Vec<StructuredPendingRelationship>,
    seen: &mut HashSet<(String, u32)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "call_expression"
        && let Some(function_node) = node.child_by_field_name("function")
        && let Some(name) = call_name(function_node, script_content)
        && !local_symbols.contains_key(&name)
    {
        let line_number = (function_node.start_position().row + start_line_offset + 1) as u32;
        if seen.insert((name.clone(), line_number)) {
            let mut target = UnresolvedTarget::simple(&name);
            if let Some(module) = imports.get(&name) {
                target.import_context = Some(module.clone());
            }
            pending.push(StructuredPendingRelationship::new(
                component.id.clone(),
                target,
                Some(component.id.clone()),
                RelationshipKind::Calls,
                base.file_path.clone(),
                line_number,
                0.8,
            ));
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_script_pending_node(
            base,
            child,
            script_content,
            start_line_offset,
            component,
            local_symbols,
            imports,
            pending,
            seen,
            child_depth,
        );
    }
}

fn collect_script_relationships(
    base: &BaseExtractor,
    section: &VueSection,
    component: &Symbol,
    local_symbols: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, RelationshipKind, u32, String)>,
) {
    let Some(tree) = parse_script_section(section) else {
        return;
    };
    visit_script_node(
        base,
        tree.root_node(),
        &section.content,
        section.start_line,
        component,
        local_symbols,
        relationships,
        seen,
        0,
    );
}

#[allow(clippy::too_many_arguments)]
fn visit_script_node(
    base: &BaseExtractor,
    node: Node,
    script_content: &str,
    start_line_offset: usize,
    component: &Symbol,
    local_symbols: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, RelationshipKind, u32, String)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "call_expression"
        && let Some(function_node) = node.child_by_field_name("function")
        && let Some(name) = call_name(function_node, script_content)
        && let Some(target) = local_symbols.get(&name)
    {
        push_relationship(
            base,
            component,
            target,
            RelationshipKind::Calls,
            (function_node.start_position().row + start_line_offset + 1) as u32,
            &name,
            None,
            seen,
            relationships,
        );
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_script_node(
            base,
            child,
            script_content,
            start_line_offset,
            component,
            local_symbols,
            relationships,
            seen,
            child_depth,
        );
    }
}

fn collect_template_relationships(
    base: &BaseExtractor,
    section: &VueSection,
    component: &Symbol,
    local_symbols: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, RelationshipKind, u32, String)>,
) {
    for (line_index, line) in section.content.lines().enumerate() {
        let line_number = section.start_line as u32 + line_index as u32 + 1;
        for captures in TEMPLATE_INTERPOLATION_RE.captures_iter(line) {
            if let Some(expression) = captures.get(1) {
                collect_template_expression_relationships(
                    base,
                    expression.as_str(),
                    expression.start(),
                    line_number,
                    component,
                    local_symbols,
                    relationships,
                    seen,
                );
            }
        }
        for captures in TEMPLATE_EVENT_RE.captures_iter(line) {
            if let Some(expression) = captures.get(1) {
                collect_template_expression_relationships(
                    base,
                    expression.as_str(),
                    expression.start(),
                    line_number,
                    component,
                    local_symbols,
                    relationships,
                    seen,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_template_expression_relationships(
    base: &BaseExtractor,
    expression: &str,
    expression_start_column: usize,
    line_number: u32,
    component: &Symbol,
    local_symbols: &HashMap<String, &Symbol>,
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, RelationshipKind, u32, String)>,
) {
    for matched in IDENTIFIER_RE
        .find_iter(expression)
        .filter(|matched| !is_template_keyword(matched.as_str()))
    {
        let name = matched.as_str();
        if let Some(target) = local_symbols.get(name) {
            push_relationship(
                base,
                component,
                target,
                RelationshipKind::References,
                line_number,
                name,
                Some(expression_start_column + matched.start()),
                seen,
                relationships,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_relationship(
    base: &BaseExtractor,
    source: &Symbol,
    target: &Symbol,
    kind: RelationshipKind,
    line_number: u32,
    reference_name: &str,
    reference_start_column: Option<usize>,
    seen: &mut HashSet<(String, String, RelationshipKind, u32, String)>,
    relationships: &mut Vec<Relationship>,
) {
    let key = (
        source.id.clone(),
        target.id.clone(),
        kind.clone(),
        line_number,
        reference_name.to_string(),
    );
    if !seen.insert(key) {
        return;
    }

    let mut metadata = HashMap::new();
    metadata.insert(
        "referenceName".to_string(),
        Value::String(reference_name.to_string()),
    );

    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}_{}",
            source.id, target.id, kind, line_number, reference_name
        ),
        from_symbol_id: source.id.clone(),
        to_symbol_id: target.id.clone(),
        kind,
        file_path: base.file_path.clone(),
        line_number,
        span: reference_start_column
            .and_then(|column| {
                crate::base::NormalizedSpan::from_line_match(
                    &base.content,
                    line_number,
                    column,
                    reference_name,
                )
            })
            .or_else(|| {
                crate::base::NormalizedSpan::from_line_occurrence(
                    &base.content,
                    line_number,
                    reference_name,
                )
            }),
        confidence: 1.0,
        metadata: Some(metadata),
    });
}

fn unique_symbols_by_name(symbols: &[Symbol]) -> HashMap<String, &Symbol> {
    let mut grouped: HashMap<&str, Vec<&Symbol>> = HashMap::new();
    for symbol in symbols {
        grouped
            .entry(symbol.name.as_str())
            .or_default()
            .push(symbol);
    }
    grouped
        .into_iter()
        .filter_map(|(name, symbols)| {
            if symbols.len() == 1 {
                Some((name.to_string(), symbols[0]))
            } else {
                None
            }
        })
        .collect()
}

fn component_symbol(symbols: &[Symbol]) -> Option<&Symbol> {
    symbols.iter().find(|symbol| {
        symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("type"))
            .and_then(Value::as_str)
            == Some("vue-sfc")
    })
}

fn parse_script_section(section: &VueSection) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    let lang = section.lang.as_deref().unwrap_or("js");
    let tree_sitter_lang = if lang == "ts" || lang == "typescript" {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    } else {
        tree_sitter_javascript::LANGUAGE.into()
    };
    parser.set_language(&tree_sitter_lang).ok()?;
    parser.parse(&section.content, None)
}

fn call_name(function_node: Node, script_content: &str) -> Option<String> {
    match function_node.kind() {
        "identifier" => Some(node_text(function_node, script_content)),
        "member_expression" => function_node
            .child_by_field_name("property")
            .map(|property| node_text(property, script_content)),
        _ => None,
    }
}

fn node_text(node: Node, content: &str) -> String {
    let bytes = content.as_bytes();
    let start = node.start_byte();
    let end = node.end_byte();
    if start < bytes.len() && end <= bytes.len() {
        String::from_utf8_lossy(&bytes[start..end]).to_string()
    } else {
        String::new()
    }
}

fn is_template_keyword(name: &str) -> bool {
    matches!(
        name,
        "true" | "false" | "null" | "undefined" | "if" | "else" | "return" | "typeof" | "new"
    )
}

fn is_component_tag(tag_name: &str) -> bool {
    tag_name
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
        || tag_name.contains('-')
}

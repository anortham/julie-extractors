//! `<script>` and `<style>` blocks run through the native JavaScript,
//! TypeScript, or CSS pipeline in host coordinates. Vue adds Composition API
//! metadata and compiler-macro symbols on top.

use super::manual_symbols::create_symbol_manual;
use super::parsing::{VueSection, script_language};
use crate::base::{
    BaseExtractor, ExtractionLevel, ExtractionResults, NormalizedSpan, Symbol, SymbolKind,
};
use crate::embedded::{extract_embedded, extract_embedded_tree};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node, Tree};

const STANDALONE_MACROS: [&str; 4] = [
    "defineExpose",
    "defineProps",
    "defineEmits",
    "defineOptions",
];

/// Symbols of a script block and its other rows, all in host coordinates.
pub(super) fn extract_script(
    base: &BaseExtractor,
    section: &VueSection,
    tree: &Tree,
) -> Option<(Vec<Symbol>, ExtractionResults)> {
    let mut results = extract_embedded_tree(
        script_language(section.lang.as_deref()),
        tree,
        &section.content,
        &base.content,
        section.content_start,
        &base.file_path,
        Path::new(""),
        ExtractionLevel::Full,
    )?;
    publish_as_host_language(&mut results, &base.language);
    let mut symbols = std::mem::take(&mut results.symbols);
    annotate_composition_api(&mut symbols, tree.root_node(), section);
    overlay_options_api(base, tree.root_node(), section, &mut symbols);
    symbols.extend(standalone_macro_symbols(base, tree.root_node(), section));
    Some((symbols, results))
}

/// Symbols of a style block and its other rows, all in host coordinates.
pub(super) fn extract_style(
    base: &BaseExtractor,
    section: &VueSection,
) -> Option<(Vec<Symbol>, ExtractionResults)> {
    let mut results = extract_embedded(
        "css",
        &section.content,
        &base.content,
        section.content_start,
        &base.file_path,
        Path::new(""),
        ExtractionLevel::Full,
    )?;
    publish_as_host_language(&mut results, &base.language);
    let symbols = std::mem::take(&mut results.symbols);
    Some((symbols, results))
}

/// Vue rows carry the `vue` language. Structural facts stay with the Vue
/// fact collector, which already scans script and style blocks.
fn publish_as_host_language(results: &mut ExtractionResults, language: &str) {
    for symbol in &mut results.symbols {
        symbol.language = language.to_string();
    }
    for identifier in &mut results.identifiers {
        identifier.language = language.to_string();
    }
    for literal in &mut results.literals {
        literal.language = language.to_string();
    }
    for region in &mut results.source_regions {
        region.language = language.to_string();
    }
    for metric in &mut results.complexity_metrics {
        metric.language = language.to_string();
    }
    for info in results.types.values_mut() {
        info.language = language.to_string();
    }
    for usage in &mut results.type_argument_usages {
        usage.language = language.to_string();
    }
    results.structural_facts.clear();
}

/// `const x = ref(0)` and friends: records the callee as `compositionApi` and
/// a coarse `type` (`ref`, `reactive`, `computed`, `props`, `emits`).
fn annotate_composition_api(symbols: &mut [Symbol], root: Node<'_>, section: &VueSection) {
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        let declaration = match statement.kind() {
            "lexical_declaration" | "variable_declaration" => statement,
            "export_statement" => match statement.child_by_field_name("declaration") {
                Some(declaration) => declaration,
                None => continue,
            },
            _ => continue,
        };
        let mut declarators = declaration.walk();
        for declarator in declaration.named_children(&mut declarators) {
            let (Some(name), Some(value)) = (
                declarator.child_by_field_name("name"),
                declarator.child_by_field_name("value"),
            ) else {
                continue;
            };
            let Some(callee) = call_callee(value, &section.content) else {
                continue;
            };
            let name_start = (section.content_start + name.start_byte()) as u32;
            let Some(symbol) = symbols.iter_mut().find(|symbol| {
                symbol.start_byte <= name_start
                    && name_start < symbol.end_byte
                    && symbol.parent_id.is_none()
            }) else {
                continue;
            };
            let kind = match callee.as_str() {
                "ref" | "shallowRef" => "ref",
                "reactive" | "shallowReactive" => "reactive",
                "computed" => "computed",
                "defineProps" | "withDefaults" => "props",
                "defineEmits" => "emits",
                _ => "variable",
            };
            let metadata = symbol.metadata.get_or_insert_with(HashMap::new);
            metadata.insert("compositionApi".to_string(), Value::String(callee));
            metadata.insert("type".to_string(), Value::String(kind.to_string()));
        }
    }
}

fn call_callee(value: Node<'_>, source: &str) -> Option<String> {
    let value = if value.kind() == "await_expression" {
        value.named_child(0)?
    } else {
        value
    };
    if value.kind() != "call_expression" {
        return None;
    }
    let callee = value.child_by_field_name("function")?;
    let text = source.get(callee.byte_range())?;
    Some(text.split('<').next().unwrap_or(text).trim().to_string())
}

/// A top-level `defineExpose({...})` and other bare compiler-macro calls.
fn standalone_macro_symbols(
    base: &BaseExtractor,
    root: Node<'_>,
    section: &VueSection,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        if statement.kind() != "expression_statement" {
            continue;
        }
        let Some(call) = statement
            .named_child(0)
            .filter(|call| call.kind() == "call_expression")
        else {
            continue;
        };
        let Some(callee) = call_callee(call, &section.content) else {
            continue;
        };
        if !STANDALONE_MACROS.contains(&callee.as_str()) {
            continue;
        }
        let Some(span) = NormalizedSpan::from_content_range(
            &base.content,
            section.content_start + call.start_byte(),
            section.content_start + call.end_byte(),
        ) else {
            continue;
        };
        let mut symbol = create_symbol_manual(
            base,
            &callee,
            SymbolKind::Function,
            span.start_line as usize,
            span.start_column as usize + 1,
            span.end_line as usize,
            span.end_column as usize + 1,
            Some(format!("{callee}()")),
            None,
            Some(HashMap::from([(
                "type".to_string(),
                Value::String("vue-macro".to_string()),
            )])),
        );
        symbol.start_column = span.start_column;
        symbol.end_column = span.end_column;
        symbol.refresh_id();
        symbols.push(symbol);
    }
    symbols
}

/// Options API structure on top of the pipeline symbols: the option groups
/// (`props`, `emits`, `data`, `computed`, `methods`, `watch`, `inject`) and
/// their members, each spanning its whole pair or method. A pipeline symbol
/// for the same member is kept and parented to its group; a missing one is
/// added.
fn overlay_options_api(
    base: &BaseExtractor,
    root: Node<'_>,
    section: &VueSection,
    symbols: &mut Vec<Symbol>,
) {
    for object in super::component::options_objects(root, &section.content) {
        let mut cursor = object.walk();
        for member in object.named_children(&mut cursor) {
            let Some(key) = member_key(member, &section.content) else {
                continue;
            };
            let group = match key.as_str() {
                "props" | "emits" | "computed" | "methods" | "watch" | "inject" | "data" => key,
                _ => continue,
            };
            let group_kind = if member.kind() == "method_definition" {
                SymbolKind::Method
            } else {
                SymbolKind::Property
            };
            let Some(group_id) = upsert_member(
                base, section, member, &group, group_kind, None, &group, symbols,
            ) else {
                continue;
            };
            let (member_kind, container) = match group.as_str() {
                "emits" => (SymbolKind::Event, member_value(member)),
                "props" | "inject" => (SymbolKind::Property, member_value(member)),
                "data" => (SymbolKind::Property, returned_object(member)),
                _ => (SymbolKind::Method, member_value(member)),
            };
            let Some(container) = container else {
                continue;
            };
            let mut members = container.walk();
            for entry in container.named_children(&mut members) {
                let name = match entry.kind() {
                    "string" => string_text(entry, &section.content),
                    "pair" | "method_definition" | "shorthand_property_identifier" => {
                        member_key(entry, &section.content)
                    }
                    _ => None,
                };
                if let Some(name) = name {
                    upsert_member(
                        base,
                        section,
                        entry,
                        &name,
                        member_kind.clone(),
                        Some(&group_id),
                        &group,
                        symbols,
                    );
                }
            }
        }
    }
}

fn member_key(member: Node<'_>, source: &str) -> Option<String> {
    let key = match member.kind() {
        "pair" => member.child_by_field_name("key")?,
        "method_definition" => member.child_by_field_name("name")?,
        "shorthand_property_identifier" => member,
        _ => return None,
    };
    let text = source.get(key.byte_range())?.trim_matches(['"', '\'']);
    (!text.is_empty()).then(|| text.to_string())
}

fn string_text(node: Node<'_>, source: &str) -> Option<String> {
    let text = source
        .get(node.byte_range())?
        .trim_matches(['"', '\'', '`']);
    (!text.is_empty()).then(|| text.to_string())
}

fn member_value(member: Node<'_>) -> Option<Node<'_>> {
    let value = member.child_by_field_name("value")?;
    matches!(value.kind(), "object" | "array").then_some(value)
}

/// The object a `data() { return {...} }` or `data: () => ({...})` returns.
fn returned_object(member: Node<'_>) -> Option<Node<'_>> {
    let mut stack = vec![member];
    while let Some(node) = stack.pop() {
        if node.kind() == "return_statement" || node.kind() == "parenthesized_expression" {
            let mut cursor = node.walk();
            if let Some(object) = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "object")
            {
                return Some(object);
            }
        }
        if node.kind() == "arrow_function"
            && let Some(body) = node.child_by_field_name("body")
            && body.kind() == "object"
        {
            return Some(body);
        }
        let mut cursor = node.walk();
        stack.extend(
            node.named_children(&mut cursor)
                .filter(|child| !matches!(child.kind(), "object" | "class_body")),
        );
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn upsert_member(
    base: &BaseExtractor,
    section: &VueSection,
    node: Node<'_>,
    name: &str,
    kind: SymbolKind,
    parent_id: Option<&str>,
    group: &str,
    symbols: &mut Vec<Symbol>,
) -> Option<String> {
    let start = (section.content_start + node.start_byte()) as u32;
    let end = (section.content_start + node.end_byte()) as u32;
    if let Some(existing) = symbols
        .iter_mut()
        .find(|symbol| symbol.name == name && start <= symbol.start_byte && symbol.start_byte < end)
    {
        if existing.parent_id.is_none() {
            existing.parent_id = parent_id.map(str::to_string);
        }
        existing
            .metadata
            .get_or_insert_with(HashMap::new)
            .insert("vueOption".to_string(), Value::String(group.to_string()));
        return Some(existing.id.clone());
    }

    let span = NormalizedSpan::from_content_range(&base.content, start as usize, end as usize)?;
    let mut metadata = HashMap::new();
    metadata.insert("vueOption".to_string(), Value::String(group.to_string()));
    let signature = base
        .content
        .get(start as usize..end as usize)
        .and_then(|text| text.lines().next())
        .map(|line| line.trim().to_string());
    let mut symbol = create_symbol_manual(
        base,
        name,
        kind,
        span.start_line as usize,
        span.start_column as usize + 1,
        span.end_line as usize,
        span.end_column as usize + 1,
        signature,
        preceding_comments(node, &section.content),
        Some(metadata),
    );
    symbol.start_column = span.start_column;
    symbol.end_column = span.end_column;
    symbol.parent_id = parent_id.map(str::to_string);
    symbol.refresh_id();
    let id = symbol.id.clone();
    symbols.push(symbol);
    Some(id)
}

/// Consecutive comment nodes right before `node`, top first.
fn preceding_comments(node: Node<'_>, source: &str) -> Option<String> {
    let mut comments = Vec::new();
    let mut current = node.prev_named_sibling();
    while let Some(sibling) = current.filter(|sibling| sibling.kind() == "comment") {
        comments.push(source.get(sibling.byte_range())?.to_string());
        current = sibling.prev_named_sibling();
    }
    comments.reverse();
    (!comments.is_empty()).then(|| comments.join("\n"))
}

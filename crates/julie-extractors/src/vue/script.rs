//! `<script>` and `<style>` blocks run through the native JavaScript,
//! TypeScript, or CSS pipeline in host coordinates. Vue adds Composition API
//! metadata and compiler-macro symbols on top.

use super::macros::MemberTypes;
use super::manual_symbols::create_symbol_manual;
use super::parsing::{VueSection, script_language};
use crate::base::{
    BaseExtractor, ExtractionLevel, ExtractionResults, Symbol, SymbolKind, TypeInfo,
};
use crate::embedded::{extract_embedded, extract_embedded_tree};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node, Tree};

const STANDALONE_MACROS: [&str; 5] = [
    "defineExpose",
    "defineProps",
    "defineEmits",
    "defineModel",
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
    let mut declared_types = annotate_composition_api(&mut symbols, tree.root_node(), section);
    overlay_options_api(
        base,
        tree.root_node(),
        section,
        &mut symbols,
        &mut declared_types,
    );
    symbols.extend(standalone_macro_symbols(base, tree.root_node(), section));
    super::macros::declare_macro_members(
        base,
        tree.root_node(),
        section,
        &mut symbols,
        &mut declared_types,
    );
    for (symbol_id, resolved_type) in declared_types {
        results.types.insert(
            symbol_id.clone(),
            TypeInfo {
                symbol_id,
                resolved_type,
                generic_params: None,
                constraints: None,
                is_inferred: false,
                language: base.language.clone(),
                metadata: None,
            },
        );
    }
    Some((symbols, results))
}

/// Symbols of a style block and its other rows, all in host coordinates.
pub(super) fn extract_style(
    base: &BaseExtractor,
    section: &VueSection,
) -> Option<(Vec<Symbol>, ExtractionResults)> {
    if !is_plain_css(section.lang.as_deref()) {
        return None;
    }
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
/// fact collector, which already scans script and style blocks; only the
/// comment markers of a block come from its own pipeline.
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
    results
        .structural_facts
        .retain(|fact| fact.pattern_id == "code.marker.v1");
    for fact in &mut results.structural_facts {
        fact.language = language.to_string();
    }
}

/// `const x = ref(0)` and friends: records the callee as `compositionApi` and
/// a coarse `type` (`ref`, `reactive`, `computed`, `props`, `emits`). Returns
/// the declared type of each `ref<T>()`, `shallowRef<T>()`, and
/// `computed<T>()` binding.
fn annotate_composition_api(
    symbols: &mut [Symbol],
    root: Node<'_>,
    section: &VueSection,
) -> MemberTypes {
    let mut declared = MemberTypes::new();
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
                    && symbol.kind != SymbolKind::Export
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
            if let Some(wrapper) = match callee.as_str() {
                "ref" => Some("Ref"),
                "shallowRef" => Some("ShallowRef"),
                "computed" => Some("ComputedRef"),
                _ => None,
            } && let Some(arguments) = unwrap_await(value)
                .child_by_field_name("type_arguments")
                .and_then(|arguments| section.content.get(arguments.byte_range()))
            {
                declared.push((symbol.id.clone(), format!("{wrapper}{arguments}")));
            }
            let metadata = symbol.metadata.get_or_insert_with(HashMap::new);
            metadata.insert("compositionApi".to_string(), Value::String(callee));
            metadata.insert("type".to_string(), Value::String(kind.to_string()));
        }
    }
    declared
}

fn unwrap_await(value: Node<'_>) -> Node<'_> {
    if value.kind() == "await_expression" {
        value.named_child(0).unwrap_or(value)
    } else {
        value
    }
}

pub(super) fn call_callee(value: Node<'_>, source: &str) -> Option<String> {
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
        let Some(symbol) = create_symbol_manual(
            base,
            &callee,
            SymbolKind::Function,
            section.content_start + call.start_byte(),
            section.content_start + call.end_byte(),
            Some(format!("{callee}()")),
            None,
            Some(HashMap::from([(
                "type".to_string(),
                Value::String("vue-macro".to_string()),
            )])),
        ) else {
            continue;
        };
        symbols.push(symbol);
    }
    symbols
}

/// Options API structure on top of the pipeline symbols: the option groups
/// (`props`, `emits`, `data`, `computed`, `methods`, `watch`, `inject`) and
/// their members, and the `setup` method, each spanning its whole pair or method. A pipeline symbol
/// for the same member is kept and parented to its group; a missing one is
/// added.
fn overlay_options_api(
    base: &BaseExtractor,
    root: Node<'_>,
    section: &VueSection,
    symbols: &mut Vec<Symbol>,
    declared_types: &mut MemberTypes,
) {
    for object in super::component::options_objects(root, &section.content) {
        let mut cursor = object.walk();
        for member in object.named_children(&mut cursor) {
            let Some(key) = member_key(member, &section.content) else {
                continue;
            };
            let group = match key.as_str() {
                "props" | "emits" | "computed" | "methods" | "watch" | "inject" | "data" => key,
                "setup" => {
                    upsert_member(
                        base,
                        section,
                        member,
                        "setup",
                        SymbolKind::Method,
                        None,
                        "setup",
                        symbols,
                    );
                    continue;
                }
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
                if let Some(name) = name
                    && let Some(id) = upsert_member(
                        base,
                        section,
                        entry,
                        &name,
                        member_kind.clone(),
                        Some(&group_id),
                        &group,
                        symbols,
                    )
                    && group == "props"
                    && let Some(prop_type) = entry
                        .child_by_field_name("value")
                        .and_then(|value| super::macros::prop_type(value, &section.content))
                {
                    declared_types.push((id, prop_type));
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
pub(super) fn upsert_member(
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
        start as usize,
        end as usize,
        signature,
        preceding_comments(node, &section.content),
        Some(metadata),
    )?;
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

/// Style blocks in plain CSS. No SCSS, Sass, Less, or Stylus grammar is
/// pinned, and the CSS grammar misreads their syntax, so those blocks yield no
/// CSS rows (as standalone `.scss` files are not extracted).
pub(crate) fn is_plain_css(lang: Option<&str>) -> bool {
    lang.is_none_or(|lang| {
        let lang = lang.trim().to_ascii_lowercase();
        lang.is_empty() || lang == "css" || lang == "postcss"
    })
}

use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::super::types::{StructuralFact, Symbol};
use super::helpers::{fact_for_node, insert_string, insert_string_array};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const RUST_BENCHMARK_PATTERN_ID: &str = "rust.benchmark.v1";
const CAPTURE_NAME: &str = "benchmark";

type FunctionSymbolIndex<'a> = HashMap<(u32, &'a str), &'a Symbol>;

pub(super) fn collect_rust_benchmark_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    if language != "rust" {
        return Vec::new();
    }
    let function_symbols = symbols
        .iter()
        .map(|symbol| ((symbol.start_byte, symbol.name.as_str()), symbol))
        .collect();
    let mut facts = Vec::new();
    walk_benchmarks(
        tree.root_node(),
        file_path,
        content,
        &function_symbols,
        0,
        &mut facts,
    );
    facts
}

fn walk_benchmarks(
    node: Node,
    file_path: &str,
    content: &str,
    function_symbols: &FunctionSymbolIndex<'_>,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "attribute_item" => {
            if let Some(fact) = function_benchmark_fact(node, file_path, content, function_symbols)
            {
                facts.push(fact);
            }
        }
        "macro_invocation" => {
            if let Some(fact) = criterion_registration_fact(node, file_path, content) {
                facts.push(fact);
            }
            return;
        }
        "macro_definition" => return,
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_benchmarks(
            child,
            file_path,
            content,
            function_symbols,
            child_depth,
            facts,
        );
    }
}

fn function_benchmark_fact(
    attribute_item: Node,
    file_path: &str,
    content: &str,
    function_symbols: &FunctionSymbolIndex<'_>,
) -> Option<StructuralFact> {
    let attribute = named_child_of_kind(attribute_item, "attribute")?;
    let path = attribute.named_child(0)?;
    let path_text = content.get(path.start_byte()..path.end_byte())?;
    let harness = match path_text {
        "bench" => "libtest",
        "divan::bench" => "divan",
        _ => return None,
    };
    let function = following_function_item(attribute_item)?;
    let name_node = function.child_by_field_name("name")?;
    let target_name = content.get(name_node.start_byte()..name_node.end_byte())?;
    let symbol = function_symbols.get(&(function.start_byte() as u32, target_name))?;
    let mut metadata = benchmark_metadata(harness, "function_attribute");
    insert_string(&mut metadata, "target_name", target_name);
    let mut fact = fact_for_node(
        file_path,
        "rust",
        RUST_BENCHMARK_PATTERN_ID,
        CAPTURE_NAME,
        attribute_item,
        metadata,
    );
    fact.containing_symbol_id = Some(symbol.id.clone());
    Some(fact)
}

fn following_function_item(attribute_item: Node) -> Option<Node> {
    let mut sibling = attribute_item.next_named_sibling()?;
    while matches!(
        sibling.kind(),
        "attribute_item" | "line_comment" | "block_comment"
    ) {
        sibling = sibling.next_named_sibling()?;
    }
    (sibling.kind() == "function_item").then_some(sibling)
}

fn criterion_registration_fact(
    invocation: Node,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let macro_path = invocation.child_by_field_name("macro")?;
    let macro_name = criterion_macro_name(macro_path, invocation, content)?;
    let arguments = named_child_of_kind(invocation, "token_tree")?;
    let metadata = match macro_name {
        "criterion_group" => {
            let (group_name, targets) = criterion_group_arguments(arguments, content)?;
            let mut metadata = benchmark_metadata("criterion", "group_registration");
            insert_string(&mut metadata, "group_name", &group_name);
            if !targets.is_empty() {
                insert_string_array(&mut metadata, "targets", targets);
            }
            metadata
        }
        "criterion_main" => {
            let targets = path_list(arguments, content);
            if targets.is_empty() {
                return None;
            }
            let mut metadata = benchmark_metadata("criterion", "entry_point_registration");
            insert_string_array(&mut metadata, "targets", targets);
            metadata
        }
        _ => return None,
    };
    Some(fact_for_node(
        file_path,
        "rust",
        RUST_BENCHMARK_PATTERN_ID,
        CAPTURE_NAME,
        invocation,
        metadata,
    ))
}

fn criterion_macro_name(macro_path: Node, invocation: Node, content: &str) -> Option<&'static str> {
    if macro_path.kind() == "scoped_identifier" {
        let prefix = macro_path.child_by_field_name("path")?;
        let suffix = macro_path.child_by_field_name("name")?;
        if content.get(prefix.start_byte()..prefix.end_byte())? != "criterion" {
            return None;
        }
        return criterion_macro_source_name(content.get(suffix.start_byte()..suffix.end_byte())?);
    }
    if macro_path.kind() != "identifier" {
        return None;
    }
    let local_name = content.get(macro_path.start_byte()..macro_path.end_byte())?;
    if local_macro_shadows(invocation, local_name, content) {
        return None;
    }
    imported_criterion_macro(invocation, local_name, content)
}

fn criterion_macro_source_name(name: &str) -> Option<&'static str> {
    match name {
        "criterion_group" => Some("criterion_group"),
        "criterion_main" => Some("criterion_main"),
        _ => None,
    }
}

fn imported_criterion_macro(
    invocation: Node,
    local_name: &str,
    content: &str,
) -> Option<&'static str> {
    for scope in lexical_scopes(invocation) {
        let mut bindings = HashMap::new();
        let mut cursor = scope.walk();
        for declaration in scope
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "use_declaration")
        {
            if let Some(argument) = declaration.child_by_field_name("argument") {
                collect_use_bindings(argument, "", content, &mut bindings, 0);
            }
        }
        if let Some(binding) = bindings.get(local_name) {
            return *binding;
        }
    }
    None
}

fn collect_use_bindings(
    node: Node,
    prefix: &str,
    content: &str,
    bindings: &mut HashMap<String, Option<&'static str>>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "scoped_use_list" => {
            let prefix = node
                .child_by_field_name("path")
                .and_then(|path| content.get(path.start_byte()..path.end_byte()))
                .map(|path| join_use_path(prefix, path))
                .unwrap_or_else(|| prefix.to_string());
            if let Some(list) = node.child_by_field_name("list") {
                let Some(child_depth) = child_tree_depth(depth) else {
                    return;
                };
                collect_use_bindings(list, &prefix, content, bindings, child_depth);
            }
        }
        "use_list" => {
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_use_bindings(child, prefix, content, bindings, child_depth);
            }
        }
        "use_as_clause" => {
            let (Some(path_node), Some(alias_node)) = (
                node.child_by_field_name("path"),
                node.child_by_field_name("alias"),
            ) else {
                return;
            };
            let (Some(path), Some(alias)) = (
                content.get(path_node.start_byte()..path_node.end_byte()),
                content.get(alias_node.start_byte()..alias_node.end_byte()),
            ) else {
                return;
            };
            bind_use_path(bindings, join_use_path(prefix, path), alias);
        }
        "use_wildcard" => {
            let Some(path) = named_child_of_kind(node, "identifier")
                .or_else(|| node.named_child(0))
                .and_then(|path| content.get(path.start_byte()..path.end_byte()))
            else {
                return;
            };
            let path = join_use_path(prefix, path);
            for name in ["criterion_group", "criterion_main"] {
                bind_use_path(bindings, join_use_path(&path, name), name);
            }
        }
        "scoped_identifier" => {
            let Some(name) = node.child_by_field_name("name") else {
                return;
            };
            let (Some(name), Some(path)) = (
                content.get(name.start_byte()..name.end_byte()),
                content.get(node.start_byte()..node.end_byte()),
            ) else {
                return;
            };
            bind_use_path(bindings, join_use_path(prefix, path), name);
        }
        "identifier" => {
            let Some(name) = content.get(node.start_byte()..node.end_byte()) else {
                return;
            };
            bind_use_path(bindings, join_use_path(prefix, name), name);
        }
        _ => {}
    }
}

fn bind_use_path(
    bindings: &mut HashMap<String, Option<&'static str>>,
    path: String,
    local_name: &str,
) {
    let macro_name = criterion_macro_source_name(path.rsplit("::").next().unwrap_or_default())
        .filter(|_| path.starts_with("criterion::"));
    bindings
        .entry(local_name.to_string())
        .and_modify(|existing| {
            if *existing != macro_name {
                *existing = None;
            }
        })
        .or_insert(macro_name);
}

fn join_use_path(prefix: &str, path: &str) -> String {
    let path = path.trim_start_matches("::");
    if prefix.is_empty() {
        path.to_string()
    } else {
        format!("{prefix}::{path}")
    }
}

fn local_macro_shadows(invocation: Node, name: &str, content: &str) -> bool {
    lexical_scopes(invocation).into_iter().any(|scope| {
        let mut cursor = scope.walk();
        scope.named_children(&mut cursor).any(|child| {
            child.kind() == "macro_definition"
                && child.start_byte() < invocation.start_byte()
                && child
                    .child_by_field_name("name")
                    .and_then(|name| content.get(name.start_byte()..name.end_byte()))
                    == Some(name)
        })
    })
}

fn lexical_scopes<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    let mut scopes = Vec::new();
    let mut ancestor = node.parent();
    while let Some(node) = ancestor {
        let module_scope = node.kind() == "declaration_list"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "mod_item");
        if matches!(node.kind(), "source_file" | "block") || module_scope {
            scopes.push(node);
        }
        if module_scope {
            break;
        }
        ancestor = node.parent();
    }
    scopes
}

fn criterion_group_arguments(token_tree: Node, content: &str) -> Option<(String, Vec<String>)> {
    let children = direct_children(token_tree);
    if children.iter().any(|child| child.kind() == ";") {
        let clauses = split_nodes(children, ";");
        let group_name = clauses
            .iter()
            .find_map(|clause| assignment_value(clause, "name", content))
            .and_then(|value| single_path(&value, content))?;
        let targets = clauses
            .iter()
            .find_map(|clause| assignment_value(clause, "targets", content))
            .map(|value| path_list_from_nodes(value, content))
            .unwrap_or_default();
        return Some((group_name, targets));
    }
    let mut arguments = split_nodes(children, ",").into_iter();
    let group_name = single_path(&arguments.next()?, content)?;
    let targets = arguments
        .flat_map(|segment| path_list_from_nodes(segment, content))
        .collect();
    Some((group_name, targets))
}

fn assignment_value<'tree>(
    nodes: &[Node<'tree>],
    name: &str,
    content: &str,
) -> Option<Vec<Node<'tree>>> {
    let equals = nodes.iter().position(|node| node.kind() == "=")?;
    let key = nodes[..equals]
        .iter()
        .find(|node| node.is_named())
        .and_then(|node| content.get(node.start_byte()..node.end_byte()))?;
    (key == name).then(|| nodes[equals + 1..].to_vec())
}

fn path_list(token_tree: Node, content: &str) -> Vec<String> {
    path_list_from_nodes(direct_children(token_tree), content)
}

fn path_list_from_nodes<'tree>(nodes: Vec<Node<'tree>>, content: &str) -> Vec<String> {
    split_nodes(nodes, ",")
        .iter()
        .filter_map(|segment| single_path(segment, content))
        .collect()
}

fn single_path<'tree>(nodes: &[Node<'tree>], content: &str) -> Option<String> {
    let mut named = nodes.iter().filter(|node| node.is_named());
    let path = named.next()?;
    if named.next().is_some() || !matches!(path.kind(), "identifier" | "scoped_identifier") {
        return None;
    }
    content
        .get(path.start_byte()..path.end_byte())
        .map(str::to_string)
}

fn direct_children<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

fn split_nodes<'tree>(nodes: Vec<Node<'tree>>, separator: &str) -> Vec<Vec<Node<'tree>>> {
    let mut segments = Vec::new();
    let mut segment = Vec::new();
    for node in nodes {
        if node.kind() == separator {
            if !segment.is_empty() {
                segments.push(std::mem::take(&mut segment));
            }
        } else {
            segment.push(node);
        }
    }
    if !segment.is_empty() {
        segments.push(segment);
    }
    segments
}

fn benchmark_metadata(harness: &str, registration_kind: &str) -> HashMap<String, Value> {
    let mut metadata = HashMap::from([(
        "pattern_version".to_string(),
        Value::Number(Number::from(1)),
    )]);
    insert_string(&mut metadata, "query_family", "testing");
    insert_string(&mut metadata, "harness", harness);
    insert_string(&mut metadata, "registration_kind", registration_kind);
    metadata
}

fn named_child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

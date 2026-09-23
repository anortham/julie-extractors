use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

use super::helpers::extract_variable_declaration_annotations;
use super::imports;
use super::type_facts;

const VALUE_SUMMARY_CHARS: usize = 80;

/// Extract a `var`/`const` declaration. The initializer node decides what it
/// declares: a container (`struct`, `union`, `enum`, `opaque`), an error set, a
/// function type, an `@import`, or a plain value.
pub(super) fn extract_variable(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
    is_public_fn: fn(&BaseExtractor, Node) -> bool,
) -> Option<Symbol> {
    let is_const = type_facts::has_keyword(node, "const");
    if !is_const && !type_facts::has_keyword(node, "var") {
        return None;
    }
    let is_public = is_public_fn(base, node);
    let value = type_facts::initializer_node(node);

    if value.is_some_and(|value| is_import_value(base, value)) {
        let node_text = base.get_node_text(&node);
        let mut symbol =
            imports::extract_import_variable(base, node, parent_id, is_public, &node_text)?;
        set_value_body(base, &mut symbol, value);
        return Some(symbol);
    }

    let name_node = base.find_child_by_type(&node, "identifier")?;
    let name = base.get_node_text(&name_node);
    let keyword = if is_const { "const" } else { "var" };
    let declaration = Declaration {
        node,
        name,
        keyword,
        parent_id,
        visibility: declared_visibility(node, is_public),
    };

    match value {
        Some(value) => match value.kind() {
            "struct_declaration" | "opaque_declaration" => Some(extract_container(
                base,
                declaration,
                value,
                SymbolKind::Struct,
            )),
            "union_declaration" => Some(extract_container(
                base,
                declaration,
                value,
                SymbolKind::Union,
            )),
            "enum_declaration" => Some(extract_container(
                base,
                declaration,
                value,
                SymbolKind::Enum,
            )),
            "function_signature" => Some(extract_function_type(base, declaration, value)),
            _ if is_error_set_value(value) => Some(extract_error_set(base, declaration, value)),
            _ => Some(extract_standard_variable(base, declaration, is_const)),
        },
        None => Some(extract_standard_variable(base, declaration, is_const)),
    }
}

struct Declaration<'tree, 'a> {
    node: Node<'tree>,
    name: String,
    keyword: &'static str,
    parent_id: Option<&'a String>,
    visibility: Visibility,
}

fn declared_visibility(node: Node, is_public: bool) -> Visibility {
    if is_public || type_facts::has_keyword(node, "export") {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

/// `@import("x")`, or a member chain rooted at one
/// (`@import("std").mem.Allocator`).
fn is_import_value(base: &BaseExtractor, value: Node) -> bool {
    let mut node = value;
    while node.kind() == "field_expression" {
        match node.child_by_field_name("object") {
            Some(object) => node = object,
            None => return false,
        }
    }
    node.kind() == "builtin_function"
        && node
            .named_child(0)
            .is_some_and(|name| base.get_node_text(&name) == "@import")
}

/// `error{ .. }`, or an `||` merge with at least one literal error set.
fn is_error_set_value(value: Node) -> bool {
    match value.kind() {
        "error_set_declaration" => true,
        "binary_expression" => value
            .named_children(&mut value.walk())
            .any(is_error_set_value),
        _ => false,
    }
}

fn set_value_body(base: &BaseExtractor, symbol: &mut Symbol, value: Option<Node>) {
    let span =
        value.and_then(|value| base.span_for_byte_range(value.start_byte(), value.end_byte()));
    base.set_body_span(symbol, span);
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The container keywords before the member list: `struct`, `packed struct`,
/// `enum(u8)`, `union(enum)`, `opaque`.
fn container_header(base: &BaseExtractor, value: Node) -> String {
    let end = value
        .children(&mut value.walk())
        .find(|child| child.kind() == "{")
        .map_or(value.end_byte(), |brace| brace.start_byte());
    collapse_whitespace(&base.content[value.start_byte()..end])
}

fn extract_container(
    base: &mut BaseExtractor,
    declaration: Declaration,
    value: Node,
    kind: SymbolKind,
) -> Symbol {
    let header = container_header(base, value);
    let metadata = (value.kind() == "opaque_declaration")
        .then(|| HashMap::from([("isOpaque".to_string(), serde_json::Value::Bool(true))]));
    let doc_comment = base.extract_documentation(&declaration.node);
    let mut symbol = base.create_symbol(
        &declaration.node,
        declaration.name.clone(),
        kind,
        SymbolOptions {
            signature: Some(format!(
                "{} {} = {}",
                declaration.keyword, declaration.name, header
            )),
            visibility: Some(declaration.visibility),
            parent_id: declaration.parent_id.cloned(),
            metadata,
            doc_comment,
            annotations: Vec::new(),
        },
    );
    set_value_body(base, &mut symbol, Some(value));
    symbol
}

fn extract_error_set(base: &mut BaseExtractor, declaration: Declaration, value: Node) -> Symbol {
    let set = if value.kind() == "error_set_declaration" {
        "error{...}".to_string()
    } else {
        value
            .named_children(&mut value.walk())
            .map(|operand| {
                if operand.kind() == "error_set_declaration" {
                    "error{...}".to_string()
                } else {
                    collapse_whitespace(&base.get_node_text(&operand))
                }
            })
            .collect::<Vec<_>>()
            .join(" || ")
    };
    let metadata = HashMap::from([("isErrorSet".to_string(), serde_json::Value::Bool(true))]);
    let doc_comment = base.extract_documentation(&declaration.node);
    let mut symbol = base.create_symbol(
        &declaration.node,
        declaration.name.clone(),
        SymbolKind::Enum,
        SymbolOptions {
            signature: Some(format!(
                "{} {} = {}",
                declaration.keyword, declaration.name, set
            )),
            visibility: Some(declaration.visibility),
            parent_id: declaration.parent_id.cloned(),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    );
    set_value_body(base, &mut symbol, Some(value));
    symbol
}

fn extract_function_type(
    base: &mut BaseExtractor,
    declaration: Declaration,
    value: Node,
) -> Symbol {
    let fn_type = collapse_whitespace(&base.get_node_text(&value));
    let metadata = HashMap::from([("isFunctionType".to_string(), serde_json::Value::Bool(true))]);
    let doc_comment = base.extract_documentation(&declaration.node);
    let mut symbol = base.create_symbol(
        &declaration.node,
        declaration.name.clone(),
        SymbolKind::Type,
        SymbolOptions {
            signature: Some(format!(
                "{} {} = {}",
                declaration.keyword, declaration.name, fn_type
            )),
            visibility: Some(declaration.visibility),
            parent_id: declaration.parent_id.cloned(),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    );
    base.set_body_span(&mut symbol, None);
    symbol
}

/// The names after the first in a destructuring declaration
/// (`const q, const r = .{ .. };` declares `r` too).
pub(super) fn extract_destructured_names(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
    is_public_fn: fn(&BaseExtractor, Node) -> bool,
) -> Vec<Symbol> {
    if node.kind() != "variable_declaration" {
        return Vec::new();
    }
    let is_public = is_public_fn(base, node);
    let mut cursor = node.walk();
    let declared: Vec<(Node, bool)> = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "identifier")
        .filter_map(|name| {
            let keyword = name.prev_sibling()?;
            matches!(keyword.kind(), "const" | "var").then(|| (name, keyword.kind() == "const"))
        })
        .collect();
    declared
        .into_iter()
        .skip(1)
        .map(|(name_node, is_const)| {
            let declaration = Declaration {
                node,
                name: base.get_node_text(&name_node),
                keyword: if is_const { "const" } else { "var" },
                parent_id,
                visibility: declared_visibility(node, is_public),
            };
            extract_standard_variable(base, declaration, is_const)
        })
        .collect()
}

/// `pub const max_size: usize`, from the declaration up to its `=`. Without a
/// stated type the signature shows the value after `=`, never as the type.
fn standard_signature(base: &BaseExtractor, declaration: &Declaration) -> String {
    let node = declaration.node;
    let declared_names = node
        .children(&mut node.walk())
        .filter(|child| {
            child.kind() == "identifier"
                && child
                    .prev_sibling()
                    .is_some_and(|keyword| matches!(keyword.kind(), "const" | "var"))
        })
        .count();
    if declared_names > 1 {
        return format!("{} {}", declaration.keyword, declaration.name);
    }
    let children: Vec<Node> = node.children(&mut node.walk()).collect();
    let equals = children.iter().find(|child| child.kind() == "=");
    let header_end = equals
        .map(|equals| equals.start_byte())
        .or_else(|| {
            children
                .iter()
                .find(|child| child.kind() == ";")
                .map(|semicolon| semicolon.start_byte())
        })
        .unwrap_or(node.end_byte());
    let header = collapse_whitespace(&base.content[node.start_byte()..header_end]);
    if node.child_by_field_name("type").is_some() {
        return header;
    }
    match type_facts::initializer_node(node) {
        Some(value) => format!("{header} = {}", value_summary(base, value)),
        None => header,
    }
}

fn value_summary(base: &BaseExtractor, value: Node) -> String {
    let text = base.get_node_text(&value);
    let mut lines = text.lines();
    let first = collapse_whitespace(lines.next().unwrap_or_default());
    let summary = BaseExtractor::truncate_string(&first, VALUE_SUMMARY_CHARS);
    if lines.next().is_some() && !summary.ends_with("...") {
        format!("{summary} ...")
    } else {
        summary
    }
}

fn extract_standard_variable(
    base: &mut BaseExtractor,
    declaration: Declaration,
    is_const: bool,
) -> Symbol {
    let node = declaration.node;
    let symbol_kind = if !is_const || type_facts::nearest_symbol_ancestor_is_callable(node) {
        SymbolKind::Variable
    } else {
        SymbolKind::Constant
    };
    let signature = standard_signature(base, &declaration);
    let annotations = extract_variable_declaration_annotations(base, node);
    let doc_comment = base.extract_documentation(&node);
    let mut symbol = base.create_symbol(
        &node,
        declaration.name,
        symbol_kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(declaration.visibility),
            parent_id: declaration.parent_id.cloned(),
            metadata: None,
            doc_comment,
            annotations,
        },
    );
    let value = type_facts::initializer_node(node);
    set_value_body(base, &mut symbol, value);
    if let Some(declared_type) = node.child_by_field_name("type") {
        type_facts::record_declared_type(base, &symbol.id, declared_type);
    } else if let Some(value) = value {
        type_facts::record_initializer_type(base, &symbol.id, value);
    }
    symbol
}

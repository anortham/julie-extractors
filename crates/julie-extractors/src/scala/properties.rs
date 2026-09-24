//! Property extraction for Scala (val/var)
//!
//! Handles immutable vals and mutable vars.

use super::helpers;
use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Where a `val`/`var` lives decides its kind: a local inside a callable,
/// block or lambda is a `variable` with no visibility, a member of a type body
/// is a `property`, and a top-level `val` is a `constant` (a top-level `var`
/// stays a `variable`).
#[derive(Clone, Copy, PartialEq)]
enum BindingScope {
    Local,
    Member,
    TopLevel,
}

fn binding_scope(node: &Node) -> BindingScope {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "function_definition"
            | "function_declaration"
            | "block"
            | "indented_block"
            | "lambda_expression"
            | "case_block" => return BindingScope::Local,
            "class_definition" | "object_definition" | "trait_definition" | "enum_definition"
            | "given_definition" | "package_object" => return BindingScope::Member,
            _ => current = ancestor.parent(),
        }
    }
    BindingScope::TopLevel
}

/// The names a `val`/`var` binds: `val a`, `val a, b`, `val (a, b)`,
/// `val Config(user, pass)`. A pattern binds its lowercase identifiers;
/// a capitalized identifier in a pattern is a stable reference.
fn bound_name_nodes<'tree>(base: &BaseExtractor, node: &Node<'tree>) -> Vec<Node<'tree>> {
    fn collect<'tree>(
        base: &BaseExtractor,
        node: Node<'tree>,
        names: &mut Vec<Node<'tree>>,
        depth: u32,
    ) {
        let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
            return;
        };
        match node.kind() {
            "identifier" => {
                let binds = base
                    .get_node_text(&node)
                    .chars()
                    .next()
                    .is_some_and(|first| !first.is_uppercase());
                if binds {
                    names.push(node);
                }
            }
            "identifiers" => {
                let mut cursor = node.walk();
                names.extend(
                    node.named_children(&mut cursor)
                        .filter(|child| child.kind() == "identifier"),
                );
            }
            "tuple_pattern" | "case_class_pattern" | "infix_pattern" | "typed_pattern"
            | "capture_pattern" => {
                let mut cursor = node.walk();
                for (index, child) in node.named_children(&mut cursor).enumerate() {
                    let is_type = node
                        .child_by_field_name("type")
                        .is_some_and(|type_node| type_node.id() == child.id());
                    let is_operator = node
                        .child_by_field_name("operator")
                        .is_some_and(|operator| operator.id() == child.id());
                    if is_type || is_operator || (node.kind() == "typed_pattern" && index > 0) {
                        continue;
                    }
                    collect(base, child, names, child_depth);
                }
            }
            _ => {}
        }
    }

    let Some(pattern) = node
        .child_by_field_name("pattern")
        .or_else(|| node.child_by_field_name("name"))
    else {
        return Vec::new();
    };
    if pattern.kind() == "identifier" {
        return vec![pattern];
    }
    let mut names = Vec::new();
    collect(base, pattern, &mut names, 0);
    names
}

/// Extract a Scala `val` or `var` as one symbol per bound name. A single
/// name spans the whole definition; each name of a multi-name or pattern
/// definition spans its own identifier.
pub(super) fn extract_bindings(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
    return_types: &type_facts::ReturnTypeIndex,
) -> Vec<Symbol> {
    let is_var = matches!(node.kind(), "var_definition" | "var_declaration");
    let keyword = if is_var { "var" } else { "val" };
    let name_nodes = bound_name_nodes(base, node);
    if name_nodes.is_empty() {
        return Vec::new();
    }
    let single = name_nodes.len() == 1
        && node
            .child_by_field_name("pattern")
            .or_else(|| node.child_by_field_name("name"))
            .is_some_and(|pattern| pattern.id() == name_nodes[0].id());

    let modifiers = helpers::extract_modifiers(base, node);
    let annotations = helpers::extract_annotations(base, node);
    let declared_type = node
        .child_by_field_name("type")
        .map(|type_node| base.get_node_text(&type_node));
    let scope = binding_scope(node);
    let symbol_kind = match (scope, is_var) {
        (BindingScope::Local, _) | (BindingScope::TopLevel, true) => SymbolKind::Variable,
        (BindingScope::Member, _) => SymbolKind::Property,
        (BindingScope::TopLevel, false) => SymbolKind::Constant,
    };
    let visibility =
        (scope != BindingScope::Local).then(|| helpers::determine_visibility(&modifiers));
    let is_lazy = modifiers.iter().any(|modifier| modifier == "lazy");
    let sig_prefix = modifiers
        .iter()
        .filter(|modifier| !helpers::is_access_modifier(modifier))
        .map(String::as_str)
        .chain(std::iter::once(keyword))
        .collect::<Vec<_>>()
        .join(" ");
    let pattern_text = node
        .child_by_field_name("pattern")
        .map(|pattern| base.get_node_text(&pattern));
    let doc_comment = base.find_doc_comment(node);

    let mut symbols = Vec::new();
    for name_node in name_nodes {
        let name = base.get_node_text(&name_node);
        let bound = if single {
            name.clone()
        } else {
            pattern_text.clone().unwrap_or_else(|| name.clone())
        };
        let mut signature = format!("{sig_prefix} {bound}");
        if let Some(declared_type) = &declared_type {
            signature.push_str(&format!(": {declared_type}"));
        }

        let type_label = match (symbol_kind == SymbolKind::Property, is_var) {
            (true, _) => "property",
            (false, true) => "var",
            (false, false) => "val",
        };
        let mut metadata = HashMap::from([
            ("type".to_string(), Value::String(type_label.to_string())),
            ("modifiers".to_string(), Value::String(modifiers.join(","))),
            ("binding".to_string(), Value::String(keyword.to_string())),
        ]);
        if is_lazy {
            metadata.insert("lazy".to_string(), Value::Bool(true));
        }
        if let Some(declared_type) = &declared_type {
            metadata.insert(
                "propertyType".to_string(),
                Value::String(declared_type.clone()),
            );
        }

        let span_node = if single { *node } else { name_node };
        let symbol = base.create_symbol(
            &span_node,
            name,
            symbol_kind.clone(),
            SymbolOptions {
                signature: Some(signature),
                visibility: visibility.clone(),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: Some(metadata),
                doc_comment: doc_comment.clone(),
                annotations: annotations.clone(),
            },
        );
        if single {
            record_binding_type_fact(base, &symbol.id, node, return_types);
        } else if let Some(type_node) = node.child_by_field_name("type") {
            type_facts::record_declared_type(base, &symbol.id, type_node);
        }
        symbols.push(symbol);
    }
    symbols
}

/// Extract the primary-constructor parameters of a class, enum or enum case,
/// from every parameter list, as property symbols (the primary-constructor
/// rule of `docs/decisions/2026-09-08-receiver-type-facts-wave-2.md`).
///
/// A parameter of a case class or enum case is a public `val` even without
/// the keyword. Any other parameter without `val`/`var` is private state, not
/// a public member: its signature carries no binding and its `binding`
/// metadata is `none`.
pub(super) fn extract_constructor_fields(
    base: &mut BaseExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: &str,
) {
    let implicit_val = node.kind() == "full_enum_case"
        || helpers::extract_modifiers(base, node)
            .iter()
            .any(|modifier| modifier == "case");
    let parameter_lists: Vec<Node> = node
        .children(&mut node.walk())
        .filter(|child| child.kind() == "class_parameters")
        .collect();
    for class_parameters in parameter_lists {
        for parameter in class_parameters.children(&mut class_parameters.walk()) {
            if parameter.kind() == "class_parameter" {
                extract_constructor_field(base, parameter, implicit_val, symbols, parent_id);
            }
        }
    }
}

fn extract_constructor_field(
    base: &mut BaseExtractor,
    parameter: Node,
    implicit_val: bool,
    symbols: &mut Vec<Symbol>,
    parent_id: &str,
) {
    let Some(name) = parameter
        .child_by_field_name("name")
        .map(|name_node| base.get_node_text(&name_node))
    else {
        return;
    };
    if symbols.iter().any(|symbol| {
        symbol.name == name
            && symbol.kind == SymbolKind::Property
            && symbol.parent_id.as_deref() == Some(parent_id)
    }) {
        return;
    }

    let param_modifiers = helpers::extract_modifiers(base, &parameter);
    let annotations = helpers::extract_annotations(base, &parameter);
    let explicit_binding = parameter
        .children(&mut parameter.walk())
        .find(|child| matches!(child.kind(), "val" | "var"))
        .map(|binding_node| base.get_node_text(&binding_node));
    let binding = explicit_binding.or_else(|| implicit_val.then(|| "val".to_string()));
    let property_type = parameter
        .child_by_field_name("type")
        .map(|type_node| base.get_node_text(&type_node));

    let mut signature = match &binding {
        Some(binding) => format!("{binding} {name}"),
        None => name.clone(),
    };
    if let Some(ref property_type) = property_type {
        signature.push_str(&format!(": {}", property_type));
    }

    let visibility = if binding.is_some() {
        helpers::determine_visibility(&param_modifiers)
    } else {
        crate::base::Visibility::Private
    };
    let doc_comment = base.find_doc_comment(&parameter);

    let mut metadata = HashMap::from([
        ("type".to_string(), Value::String("property".to_string())),
        (
            "binding".to_string(),
            Value::String(binding.unwrap_or_else(|| "none".to_string())),
        ),
        (
            "modifiers".to_string(),
            Value::String(param_modifiers.join(",")),
        ),
    ]);
    if let Some(ref property_type) = property_type {
        metadata.insert(
            "propertyType".to_string(),
            Value::String(property_type.clone()),
        );
    }

    let symbol = base.create_symbol(
        &parameter,
        name,
        SymbolKind::Property,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: Some(parent_id.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    );
    if let Some(type_node) = parameter.child_by_field_name("type") {
        type_facts::record_declared_type(base, &symbol.id, type_node);
    }
    symbols.push(symbol);
}

fn record_binding_type_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    node: &Node,
    return_types: &type_facts::ReturnTypeIndex,
) {
    if let Some(type_node) = node.child_by_field_name("type") {
        type_facts::record_declared_type(base, symbol_id, type_node);
    } else if let Some(value) = node.child_by_field_name("value") {
        type_facts::record_initializer_type(base, symbol_id, value, return_types);
    }
}

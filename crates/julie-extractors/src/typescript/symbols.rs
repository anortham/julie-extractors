//! Core symbol extraction logic
//!
//! This module handles the main tree traversal and symbol type routing.
//! It delegates to specialized modules for specific symbol kinds.

use super::{classes, functions, helpers, imports_exports, interfaces};
use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::javascript::test_symbols;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use crate::typescript::TypeScriptExtractor;
use tree_sitter::{Node, Tree};

/// Extract all symbols from the syntax tree
pub(super) fn extract_symbols(extractor: &mut TypeScriptExtractor, tree: &Tree) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    extractor.test_dsl_active =
        test_symbols::test_dsl_is_active(extractor.base(), tree.root_node());
    extractor.return_types =
        super::type_facts::ReturnTypeIndex::build(extractor.base(), tree.root_node());
    visit_node(extractor, tree.root_node(), &mut symbols, None, 0);
    mark_listed_exports_public(extractor, tree.root_node(), &mut symbols);
    symbols
}

/// A module-level declaration exported by name elsewhere in the file
/// (`export { local }`, `export default local`, `export = local`) is public,
/// the same as one written with an `export` wrapper.
fn mark_listed_exports_public(extractor: &TypeScriptExtractor, root: Node, symbols: &mut [Symbol]) {
    let exported = crate::javascript::exports::locally_exported_names(extractor.base(), root);
    if exported.is_empty() {
        return;
    }
    for symbol in symbols.iter_mut() {
        if symbol.parent_id.is_none()
            && symbol.visibility.is_none()
            && matches!(
                symbol.kind,
                SymbolKind::Class
                    | SymbolKind::Function
                    | SymbolKind::Variable
                    | SymbolKind::Interface
                    | SymbolKind::Type
                    | SymbolKind::Enum
                    | SymbolKind::Namespace
            )
            && exported.contains(&symbol.name)
        {
            symbol.visibility = Some(crate::base::Visibility::Public);
        }
    }
}

/// Check if a node is a direct child of an interface_body
fn is_inside_interface(node: &Node) -> bool {
    node.parent()
        .map(|p| p.kind() == "interface_body")
        .unwrap_or(false)
}

/// A member of the object type a type alias names: `type P = { a: T }`.
/// Object types in annotations, type arguments, and function types are
/// anonymous shapes, not declarations.
fn is_type_alias_member(node: &Node) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == "object_type")
        .and_then(|object_type| object_type.parent())
        .is_some_and(|owner| owner.kind() == "type_alias_declaration")
}

/// Recursively visit nodes and extract symbols based on node kind
fn visit_node(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let next_parent_id =
        extract_node_symbols(extractor, node, symbols, parent_id.as_deref()).or(parent_id);

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_node(
            extractor,
            child,
            symbols,
            next_parent_id.clone(),
            child_depth,
        );
    }
}

/// Emit the symbols `node` declares and return the parent id its children
/// take when it opens a new scope. Kept out of the recursive frame so the
/// walker stays small at the depth budget.
#[inline(never)]
fn extract_node_symbols(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) -> Option<String> {
    let symbol = match node.kind() {
        "class_declaration" | "abstract_class_declaration" | "class" => {
            classes::extract_class(extractor, node, parent_id)
        }
        "function_declaration" | "generator_function_declaration" | "function_signature" => {
            functions::extract_function(extractor, node, parent_id)
        }
        "function_expression" | "generator_function" | "arrow_function"
            if node
                .parent()
                .is_some_and(|parent| parent.kind() == "export_statement") =>
        {
            functions::extract_function(extractor, node, parent_id)
        }
        // Interface methods are extracted by extract_interface to get the
        // correct parent_id.
        "method_definition" | "abstract_method_signature" => {
            functions::extract_method(extractor, node, parent_id)
        }
        "method_signature"
            if !is_inside_interface(&node)
                && (is_type_alias_member(&node)
                    || node.parent().is_some_and(|p| p.kind() == "class_body")) =>
        {
            functions::extract_method(extractor, node, parent_id)
        }
        "variable_declarator"
            if node
                .child_by_field_name("name")
                .is_some_and(|name| matches!(name.kind(), "object_pattern" | "array_pattern")) =>
        {
            symbols.extend(functions::extract_destructured_variables(
                extractor, node, parent_id,
            ));
            None
        }
        "variable_declarator" => functions::extract_variable(extractor, node, parent_id),
        "interface_declaration" => {
            let interface_symbols = interfaces::extract_interface(extractor, node, parent_id);
            let interface_id = interface_symbols.first().map(|symbol| symbol.id.clone());
            symbols.extend(interface_symbols);
            return interface_id;
        }
        "type_alias_declaration" => interfaces::extract_type_alias(extractor, node, parent_id),
        "enum_declaration" => {
            let enum_symbols = interfaces::extract_enum(extractor, node, parent_id);
            let enum_id = enum_symbols.first().map(|symbol| symbol.id.clone());
            symbols.extend(enum_symbols);
            return enum_id;
        }
        "import_statement" | "import_declaration" => {
            symbols.extend(imports_exports::extract_import(extractor, node));
            None
        }
        "export_statement" => {
            symbols.extend(imports_exports::extract_export(extractor, node));
            None
        }
        "call_expression" if imports_exports::is_dynamic_import(node) => {
            imports_exports::extract_dynamic_import(extractor, node, parent_id)
        }
        "module" | "internal_module" => interfaces::extract_namespace(extractor, node, parent_id),
        "ambient_declaration" => {
            interfaces::extract_global_augmentation(extractor, node, parent_id)
        }
        "property_signature" if !is_inside_interface(&node) && is_type_alias_member(&node) => {
            interfaces::extract_property(extractor, node, parent_id)
        }
        "public_field_definition" | "property_definition" => {
            interfaces::extract_property(extractor, node, parent_id)
        }
        "assignment_expression" => {
            extract_constructor_property(extractor, node, symbols)
        }
        "pair" if functions::function_value(node).is_some() => {
            functions::extract_member_function(extractor, node, parent_id)
        }
        "call_expression"
            if extractor.test_dsl_active
                && test_symbols::is_test_dsl_call(extractor.base(), node) =>
        {
            let parent = symbols
                .iter()
                .rev()
                .find(|s| {
                    s.metadata
                        .as_ref()
                        .and_then(|m| m.get("test_container"))
                        .and_then(|v| v.as_bool())
                        == Some(true)
                        && s.start_byte <= node.start_byte() as u32
                        && s.end_byte >= node.end_byte() as u32
                })
                .map(|s| s.id.clone());
            test_symbols::extract_test_call(extractor.base_mut(), node, parent.as_deref())
        }
        _ => None,
    }?;

    let opens_scope = is_parent_scope_kind(&symbol.kind) || opens_member_scope(node, &symbol.kind);
    let parameter_source = matches!(
        symbol.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
    .then(|| callable_node_for(node))
    .flatten();
    let symbol_id = symbol.id.clone();
    let class_parent_id = symbol.parent_id.clone();
    let is_constructor = symbol.kind == SymbolKind::Constructor;
    symbols.push(symbol);

    if let Some(callable_node) = parameter_source {
        for (param_symbol, param_node) in crate::javascript::parameters::extract_parameter_symbols(
            extractor.base_mut(),
            callable_node,
            &symbol_id,
        ) {
            super::type_facts::record_annotation_fact(
                extractor.base_mut(),
                &param_symbol.id,
                param_node,
            );
            super::type_facts::record_destructured_binding_fact(
                extractor.base_mut(),
                &param_symbol.id,
                param_node,
            );
            symbols.push(param_symbol);
            if is_constructor
                && let Some(property) = interfaces::extract_parameter_property(
                    extractor,
                    param_node,
                    class_parent_id.as_deref(),
                )
            {
                symbols.push(property);
            }
        }
    }

    opens_scope.then_some(symbol_id)
}

/// The node whose `parameters` a callable symbol owns.
fn callable_node_for(node: Node) -> Option<Node> {
    match node.kind() {
        "variable_declarator" | "public_field_definition" | "property_definition" | "pair" => {
            functions::function_value(node)
        }
        "call_expression" => test_callback(node),
        _ => Some(node),
    }
}

/// The callback a test DSL call runs: `test("name", async ({ page }) => {})`.
fn test_callback(call: Node) -> Option<Node> {
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .filter(|argument| matches!(argument.kind(), "arrow_function" | "function_expression"))
        .last()
}

/// A variable bound to an object literal parents the object's members, and a
/// type alias of an object type parents its property signatures.
fn opens_member_scope(node: Node, kind: &SymbolKind) -> bool {
    match kind {
        SymbolKind::Variable => node
            .child_by_field_name("value")
            .map(functions::unwrap_expression)
            .is_some_and(|value| value.kind() == "object"),
        SymbolKind::Type => node
            .child_by_field_name("value")
            .is_some_and(|value| value.kind() == "object_type"),
        _ => false,
    }
}

fn is_parent_scope_kind(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Namespace
            | SymbolKind::Module
            | SymbolKind::Enum
            | SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Constructor
    )
}

/// `this.name = value` inside a class constructor declares an instance
/// property of the class, unless the class already declares the member.
fn extract_constructor_property(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    symbols: &[Symbol],
) -> Option<Symbol> {
    let left = node.child_by_field_name("left")?;
    let value = node.child_by_field_name("right")?;
    if left.kind() != "member_expression" {
        return None;
    }
    let object = left.child_by_field_name("object")?;
    if object.kind() != "this" {
        return None;
    }
    let property_name = extractor
        .base()
        .get_node_text(&left.child_by_field_name("property")?);

    let constructor = enclosing_constructor(node, &extractor.base().content)?;
    let class_body = constructor.parent()?;
    let class = class_body.parent()?;
    let class_symbol = symbols.iter().find(|symbol| {
        symbol.kind == SymbolKind::Class && symbol.start_byte == class.start_byte() as u32
    })?;

    if symbols.iter().any(|symbol| {
        symbol.parent_id.as_deref() == Some(&class_symbol.id)
            && symbol.name == property_name
            && symbol.kind != SymbolKind::Method
    }) || class_body_declares_property(class_body, &property_name, &extractor.base().content) {
        return None;
    }

    let class_id = class_symbol.id.clone();
    let metadata = std::collections::HashMap::from([(
        "isConstructorAssigned".to_string(),
        serde_json::json!(true),
    )]);
    let signature = extractor.base().get_node_text(&node);
    let visibility = helpers::extract_ts_visibility(node);
    let property = extractor.base_mut().create_symbol(
        &node,
        property_name,
        SymbolKind::Property,
        SymbolOptions {
            signature: Some(signature),
            visibility,
            parent_id: Some(class_id),
            metadata: Some(metadata),
            ..Default::default()
        },
    );
    crate::javascript::type_facts::record_new_expression_fact(
        extractor.base_mut(),
        &property.id,
        value,
        &super::type_facts::TYPE_NAME_RULES,
    );
    Some(property)
}

fn enclosing_constructor<'t>(node: Node<'t>, content: &str) -> Option<Node<'t>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "method_definition" => {
                let is_constructor = candidate
                    .child_by_field_name("name")
                    .and_then(|name| content.get(name.byte_range()))
                    == Some("constructor");
                return (is_constructor
                    && candidate
                        .parent()
                        .is_some_and(|body| body.kind() == "class_body"))
                .then_some(candidate);
            }
            "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "generator_function"
            | "generator_function_declaration"
            | "class_body" => return None,
            _ => current = candidate.parent(),
        }
    }
    None
}

fn class_body_declares_property(class_body: Node, name: &str, content: &str) -> bool {
    let mut cursor = class_body.walk();
    for child in class_body.children(&mut cursor) {
        if matches!(
            child.kind(),
            "public_field_definition" | "property_definition" | "field_definition"
        ) {
            let name_node = child
                .child_by_field_name("name")
                .or_else(|| child.child_by_field_name("property"))
                .or_else(|| child.child_by_field_name("key"));
            if let Some(n) = name_node
                && content.get(n.byte_range()) == Some(name)
            {
                return true;
            }
        }
    }
    false
}


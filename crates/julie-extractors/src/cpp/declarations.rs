//! Declaration extraction for C++ symbols
//! Handles extraction of declarations, friend declarations, and using declarations

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use tree_sitter::Node;

use super::declarators;
use super::functions;
use super::helpers;
use super::signatures;
use super::type_facts::{self, ReturnTypeIndex};
use super::visibility;

// Re-export field/multi-declaration extractors so mod.rs callers don't need to change
pub(super) use super::fields::{extract_field, extract_multi_declarations};

// Re-export visibility so existing callers (e.g. functions.rs) can use declarations::extract_cpp_visibility
pub(super) use super::visibility::extract_cpp_visibility;

/// Extract namespace declaration
pub(super) fn extract_namespace(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let name_node = node.children(&mut cursor).find(|c| {
        matches!(
            c.kind(),
            "namespace_identifier" | "nested_namespace_specifier"
        )
    })?;

    let name = base.get_node_text(&name_node);
    let signature = format!("namespace {}", name);

    let doc_comment = base.find_doc_comment(&node);

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Namespace,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract using declarations and namespace aliases
pub(super) fn extract_using(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut name = String::new();
    let mut signature = String::new();

    if node.kind() == "using_declaration" {
        let mut cursor = node.walk();
        let qualified_id_node = node
            .children(&mut cursor)
            .find(|c| c.kind() == "qualified_identifier" || c.kind() == "identifier")?;

        let full_path = base.get_node_text(&qualified_id_node);

        // Check if it's "using namespace"
        let is_namespace = node
            .children(&mut node.walk())
            .any(|c| c.kind() == "namespace");

        if is_namespace {
            name = full_path.clone();
            signature = format!("using namespace {}", full_path);
        } else {
            // Extract the last part for the symbol name
            let parts: Vec<&str> = full_path.split("::").collect();
            name = (*parts.last().unwrap_or(&full_path.as_str())).to_string();
            signature = format!("using {}", full_path);
        }
    } else if node.kind() == "namespace_alias_definition" {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();

        let alias_node = children
            .iter()
            .find(|c| c.kind() == "namespace_identifier")?;
        let target_node = children.iter().find(|c| {
            c.kind() == "nested_namespace_specifier" || c.kind() == "qualified_identifier"
        })?;

        name = base.get_node_text(alias_node);
        let target = base.get_node_text(target_node);
        signature = format!("namespace {} = {}", name, target);
    }

    if name.is_empty() {
        return None;
    }

    let doc_comment = base.find_doc_comment(&node);

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract template declaration.
/// Returns None for template classes/structs/functions (walk_children handles those).
/// Extracts template VARIABLES directly (e.g. `template<class T> constexpr T pi = T(3.14)`).
pub(super) fn extract_template(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
    symbols: &[Symbol],
    return_types: &ReturnTypeIndex,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let inner = node
        .children(&mut cursor)
        .find(|c| c.kind() != "template_parameter_list" && c.kind() != "template")?;

    // Classes/structs/functions already call extract_template_parameters() via walk_children
    match inner.kind() {
        "class_specifier"
        | "struct_specifier"
        | "union_specifier"
        | "function_definition"
        | "template_declaration" => return None,
        "declaration" if declarators::function_declarator_of(inner).is_some() => return None,
        "declaration" => {}
        _ => return None,
    }

    // Extract template parameter prefix
    let template_params = {
        let mut c2 = node.walk();
        node.children(&mut c2)
            .find(|c| c.kind() == "template_parameter_list")
            .map(|pl| format!("template{}", base.get_node_text(&pl)))
    };

    let mut symbol = extract_declaration(base, inner, parent_id, symbols, return_types)?;

    // Prepend template parameters to the signature
    if let Some(template_prefix) = template_params {
        let existing_sig = symbol.signature.unwrap_or_default();
        symbol.signature = Some(format!("{}\n{}", template_prefix, existing_sig));
    }

    Some(symbol)
}

/// Extract declaration (which may contain variables, functions, etc.)
pub(super) fn extract_declaration(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
    symbols: &[Symbol],
    return_types: &ReturnTypeIndex,
) -> Option<Symbol> {
    // Check if this is a friend declaration first
    let node_text = base.get_node_text(&node);
    let has_friend = node
        .children(&mut node.walk())
        .any(|c| c.kind() == "friend" || base.get_node_text(&c) == "friend");

    let has_friend_text = node_text.starts_with("friend") || node_text.contains(" friend ");

    if has_friend || has_friend_text {
        return extract_friend_declaration(base, node, parent_id);
    }

    // Check if this is a conversion operator (e.g., operator double())
    let operator_cast = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "operator_cast");
    if operator_cast.is_some() {
        return extract_conversion_operator(base, node, parent_id);
    }

    if let Some(func_declarator) = declarators::function_declarator_of(node) {
        let destructor_name = func_declarator
            .children(&mut func_declarator.walk())
            .find(|c| c.kind() == "destructor_name");
        if destructor_name.is_some() {
            return extract_destructor_from_declaration(base, node, func_declarator, parent_id);
        }

        let name_node = functions::extract_function_name(func_declarator)?;
        let name = base.get_node_text(&name_node);

        if functions::is_constructor(base, &name, node) {
            return extract_constructor_from_declaration(base, node, func_declarator, parent_id);
        }

        return functions::extract_function(base, func_declarator, parent_id, symbols);
    }

    let (declarator, name_node) = *declarators::object_names(node).first()?;
    Some(object_symbol(
        base,
        node,
        declarator,
        name_node,
        parent_id,
        return_types,
    ))
}

/// One variable or constant row for a name a declaration introduces.
pub(super) fn object_symbol(
    base: &mut BaseExtractor,
    node: Node,
    declarator: Node,
    name_node: Node,
    parent_id: Option<&str>,
    return_types: &ReturnTypeIndex,
) -> Symbol {
    let name = base.get_node_text(&name_node);
    let storage_class = helpers::extract_storage_class(base, node);
    let type_specifiers = helpers::extract_type_specifiers(base, node);
    let is_constant = helpers::is_constant_declaration(&storage_class, &type_specifiers);
    let is_static_member = helpers::is_static_member_variable(node, &storage_class);
    let kind = if is_constant || is_static_member {
        SymbolKind::Constant
    } else {
        SymbolKind::Variable
    };

    let signature = signatures::build_variable_signature(base, node, &name);
    let vis = visibility::extract_visibility_from_node(base, node);
    let doc_comment = base.find_doc_comment(&node);
    let metadata = helpers::scope_metadata(base, name_node);

    let symbol = base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(vis),
            parent_id: parent_id.map(String::from),
            metadata,
            doc_comment,
            annotations: Vec::new(),
        },
    );
    let direct_initialization = declarators::declarator_target(declarator)
        .and_then(|target| target.function)
        .is_some();
    if direct_initialization {
        type_facts::record_field_fact(base, &symbol.id, node, None);
    } else if declarators::declared_names(declarator).len() == 1 {
        type_facts::record_variable_fact(base, &symbol.id, node, declarator, return_types);
    }
    symbol
}

/// Extract friend declaration
pub(super) fn extract_friend_declaration(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut cursor = node.walk();

    // Look for the inner declaration node
    let inner_declaration = node
        .children(&mut cursor)
        .find(|c| c.kind() == "declaration")?;

    // Look for function_declarator in the declaration
    let function_declarator = helpers::find_function_declarator_in_node(inner_declaration)?;

    // Extract name - handle both operator_name and regular identifier
    let (name, symbol_kind) = if let Some(operator_name) = function_declarator
        .children(&mut function_declarator.walk())
        .find(|c| c.kind() == "operator_name")
    {
        // This is a friend operator
        (base.get_node_text(&operator_name), SymbolKind::Operator)
    } else {
        let identifier = function_declarator
            .children(&mut function_declarator.walk())
            .find(|c| c.kind() == "identifier")?;
        // This is a friend function
        (base.get_node_text(&identifier), SymbolKind::Function)
    };

    // Build friend signature
    let return_type = functions::declared_return_type(base, inner_declaration);
    let parameters = functions::extract_function_parameters(base, function_declarator);

    let signature = format!("friend {return_type}{name}{parameters}")
        .trim()
        .to_string();

    // Extract doc comment
    let doc_comment = base.find_doc_comment(&node);

    // Create the symbol
    let symbol = base.create_symbol(
        &node,
        name,
        symbol_kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    );

    Some(symbol)
}

// ========================================================================
// Private helpers for special declaration types
// ========================================================================

/// A conversion operator (`operator bool() const`), declared or defined in a
/// class body.
pub(super) fn extract_conversion_operator(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    // Find the operator_cast node
    let operator_cast = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "operator_cast")?;

    // Extract the target type from operator_cast
    let mut operator_name = "operator".to_string();

    let mut cursor = operator_cast.walk();
    for child in operator_cast.children(&mut cursor) {
        if matches!(
            child.kind(),
            "primitive_type" | "type_identifier" | "qualified_identifier"
        ) {
            let target_type = base.get_node_text(&child);
            operator_name.push(' ');
            operator_name.push_str(&target_type);
            break;
        }
    }

    let head_end = node
        .child_by_field_name("body")
        .map_or(node.end_byte(), |body| body.start_byte());
    let signature = base.content[node.start_byte()..head_end].trim().to_string();

    let doc_comment = base.find_doc_comment(&node);

    Some(base.create_symbol(
        &node,
        operator_name,
        SymbolKind::Operator,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(extract_cpp_visibility(base, node)),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

fn extract_destructor_from_declaration(
    base: &mut BaseExtractor,
    node: Node,
    _func_declarator: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let declaration = base.get_node_text(&node);
    let signature = declaration
        .trim_end()
        .trim_end_matches(';')
        .trim_end()
        .to_string();
    let name_start = signature.find('~')?;
    let name_end = signature[name_start..].find('(').map(|i| name_start + i)?;
    // SAFETY: Check char boundaries before slicing to prevent UTF-8 panic
    if !signature.is_char_boundary(name_start) || !signature.is_char_boundary(name_end) {
        return None;
    }
    let name = signature[name_start..name_end].to_string();

    let doc_comment = base.find_doc_comment(&node);

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Destructor,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility::extract_cpp_visibility(base, node)),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

fn extract_constructor_from_declaration(
    base: &mut BaseExtractor,
    node: Node,
    func_declarator: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = functions::extract_function_name(func_declarator)?;
    let name = base.get_node_text(&name_node);

    // Build signature
    let mut signature = String::new();

    // Add modifiers
    let modifiers = functions::extract_function_modifiers(base, node);
    if !modifiers.is_empty() {
        signature.push_str(&modifiers.join(" "));
        signature.push(' ');
    }

    // Add constructor name and parameters
    signature.push_str(&name);
    let parameters = functions::extract_function_parameters(base, func_declarator);
    signature.push_str(&parameters);

    // Check for noexcept
    let noexcept_spec = functions::extract_noexcept_specifier(base, func_declarator);
    if !noexcept_spec.is_empty() {
        signature.push(' ');
        signature.push_str(&noexcept_spec);
    }

    // Check for = delete, = default
    let children: Vec<Node> = node.children(&mut node.walk()).collect();
    for (i, child) in children.iter().enumerate() {
        if child.kind() == "=" && i + 1 < children.len() {
            let next_child = &children[i + 1];
            if matches!(next_child.kind(), "delete" | "default") {
                signature.push_str(&format!(" = {}", base.get_node_text(next_child)));
                break;
            }
        }
    }

    let doc_comment = base.find_doc_comment(&node);

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Constructor,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility::extract_cpp_visibility(base, node)),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract struct type declarations
pub(super) fn extract_struct(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
    is_public_fn: fn(&BaseExtractor, Node) -> bool,
) -> Option<Symbol> {
    let name_node = base.find_child_by_type(&node, "identifier")?;
    let name = base.get_node_text(&name_node);
    let is_public = is_public_fn(base, node);

    let signature = format!("struct {}", name);
    let visibility = if is_public {
        Visibility::Public
    } else {
        Visibility::Private
    };

    let doc_comment = base.extract_documentation(&node);

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Struct,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.cloned(),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract union type declarations
pub(super) fn extract_union(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
    is_public_fn: fn(&BaseExtractor, Node) -> bool,
) -> Option<Symbol> {
    let name_node = base.find_child_by_type(&node, "identifier")?;
    let name = base.get_node_text(&name_node);
    let is_public = is_public_fn(base, node);

    // Check if it's a union(enum) or regular union
    let node_text = base.get_node_text(&node);
    let union_type = if node_text.contains("union(enum)") {
        "union(enum)"
    } else {
        "union"
    };

    let signature = format!("{} {}", union_type, name);
    let visibility = if is_public {
        Visibility::Public
    } else {
        Visibility::Private
    };

    let doc_comment = base.extract_documentation(&node);

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Union,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.cloned(),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract enum type declarations
pub(super) fn extract_enum(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
    is_public_fn: fn(&BaseExtractor, Node) -> bool,
) -> Option<Symbol> {
    let name_node = base.find_child_by_type(&node, "identifier")?;
    let name = base.get_node_text(&name_node);
    let is_public = is_public_fn(base, node);

    let signature = format!("enum {}", name);
    let visibility = if is_public {
        Visibility::Public
    } else {
        Visibility::Private
    };

    let doc_comment = base.extract_documentation(&node);

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Enum,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.cloned(),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// An enum tag (`identifier`, `debug = 0`), parented to its enum.
pub(super) fn extract_enum_variant(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
) -> Option<Symbol> {
    let name_node = node
        .child_by_field_name("name")
        .filter(|name| !name.byte_range().is_empty())?;
    let name = base.get_node_text(&name_node);
    let signature = base
        .get_node_text(&node)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let doc_comment = base.extract_documentation(&node);
    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::EnumMember,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.cloned(),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// A member of a named error set (`const E = error{ A, B };`, also inside an
/// `error{..} || Other` merge). Inline error sets in return types declare no
/// named set, so their members are not symbols.
pub(super) fn is_named_error_set_member(node: Node) -> bool {
    let Some(set) = node
        .parent()
        .filter(|parent| parent.kind() == "error_set_declaration")
    else {
        return false;
    };
    let mut owner = set.parent();
    while let Some(parent) = owner.filter(|parent| parent.kind() == "binary_expression") {
        owner = parent.parent();
    }
    owner.is_some_and(|owner| owner.kind() == "variable_declaration")
}

pub(super) fn extract_error_set_member(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
) -> Option<Symbol> {
    let name = base.get_node_text(&node);
    let doc_comment = base.extract_documentation(&node);
    Some(base.create_symbol(
        &node,
        name.clone(),
        SymbolKind::EnumMember,
        SymbolOptions {
            signature: Some(format!("error.{name}")),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.cloned(),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract struct/container field declarations
pub(super) fn extract_struct_field(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
) -> Option<Symbol> {
    let name_node = base
        .find_child_by_type(&node, "identifier")
        .filter(|name| !name.byte_range().is_empty())?;
    let field_name = base.get_node_text(&name_node);

    let type_node = base
        .find_child_by_type(&node, "type_expression")
        .or_else(|| base.find_child_by_type(&node, "builtin_type"))
        .or_else(|| base.find_child_by_type(&node, "slice_type"))
        .or_else(|| {
            // Look for identifier after colon for type
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let colon_index = children.iter().position(|child| child.kind() == ":")?;
            children.get(colon_index + 1).copied()
        });

    let field_type = if let Some(type_node) = type_node {
        base.get_node_text(&type_node)
    } else {
        String::new()
    };

    let signature = format!("{}: {}", field_name, field_type);

    let symbol = base.create_symbol(
        &node,
        field_name,
        SymbolKind::Field,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.cloned(),
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    if let Some(declared_type) = node.child_by_field_name("type").or(type_node) {
        type_facts::record_declared_type(base, &symbol.id, declared_type);
    }
    Some(symbol)
}

/// Extract type alias declarations
pub(super) fn extract_type_alias(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
    is_public_fn: fn(&BaseExtractor, Node) -> bool,
) -> Option<Symbol> {
    let name_node = base.find_child_by_type(&node, "identifier")?;
    let name = base.get_node_text(&name_node);
    let is_public = is_public_fn(base, node);

    let signature = format!("type {}", name);
    let visibility = if is_public {
        Visibility::Public
    } else {
        Visibility::Private
    };

    let metadata = Some({
        let mut meta = HashMap::new();
        meta.insert("isTypeAlias".to_string(), serde_json::Value::Bool(true));
        meta
    });

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Type,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.cloned(),
            metadata,
            doc_comment: base.extract_documentation(&node),
            annotations: Vec::new(),
        },
    ))
}

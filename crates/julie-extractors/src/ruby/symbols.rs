use super::doc_comments::find_ruby_doc_comment;
use super::helpers::{declared_name, extract_alias_name, extract_name_from_node};
use super::signatures;
/// Symbol extraction for individual Ruby constructs
/// Handles extraction of modules, classes, methods, variables, constants, and aliases
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::test_detection::apply_callable_test_metadata;
use std::collections::HashMap;
use tree_sitter::Node;

/// The visibility and class-method state in force where a member is declared.
pub(super) struct MemberScope {
    pub(super) visibility: Visibility,
    /// Inside `class << self`, where every `def` defines a class method.
    pub(super) class_method: bool,
}

/// `isStatic: true` marks a class method: `def self.x`, or a `def` inside
/// `class << self`.
pub(super) fn class_method_metadata(metadata: &mut HashMap<String, serde_json::Value>) {
    metadata.insert("isStatic".to_string(), serde_json::Value::Bool(true));
}

/// Extract a module symbol
pub(super) fn extract_module(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = declared_name(base, node)?;
    let written_name = extract_name_from_node(node, |n| base.get_node_text(n), "name")
        .unwrap_or_else(|| name.clone());
    let qualified_name =
        super::helpers::build_qualified_name(node, &written_name, |n| base.get_node_text(n));

    let signature =
        signatures::build_module_signature(&node, &written_name, |n| base.get_node_text(n));
    let doc_comment = find_ruby_doc_comment(base, node);
    let metadata = (qualified_name != name).then(|| {
        HashMap::from([(
            "qualifiedName".to_string(),
            serde_json::Value::String(qualified_name),
        )])
    });

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Module,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id,
            metadata,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract a class symbol
pub(super) fn extract_class(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = declared_name(base, node)?;
    let written_name = extract_name_from_node(node, |n| base.get_node_text(n), "name")
        .unwrap_or_else(|| name.clone());
    let qualified_name =
        super::helpers::build_qualified_name(node, &written_name, |n| base.get_node_text(n));

    let signature =
        signatures::build_class_signature(&node, &written_name, |n| base.get_node_text(n));
    let doc_comment = find_ruby_doc_comment(base, node);

    let mut metadata = HashMap::new();
    if qualified_name != name {
        metadata.insert(
            "qualifiedName".to_string(),
            serde_json::Value::String(qualified_name),
        );
    }
    if let Some(base_type) = extract_superclass_name(base, node) {
        metadata.insert(
            "base_types".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::String(base_type)]),
        );
    }

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Class,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id,
            metadata: (!metadata.is_empty()).then_some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Name of the single class a Ruby class inherits from.
///
/// The grammar wraps the superclass in a `superclass` node that also holds the
/// `<` token, so the name is the constant or `::`-scoped constant inside it. A
/// versioned superclass such as `ActiveRecord::Migration[7.1]` names
/// `ActiveRecord::Migration`. A computed superclass such as
/// `class Row < Struct.new(:a)` yields no name.
pub(super) fn extract_superclass_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let superclass = node.child_by_field_name("superclass")?;
    let mut cursor = superclass.walk();
    let value = superclass.named_children(&mut cursor).next()?;
    superclass_constant(base, value)
}

fn superclass_constant(base: &BaseExtractor, node: Node) -> Option<String> {
    match node.kind() {
        "constant" | "scope_resolution" => Some(base.get_node_text(&node)),
        "element_reference" => superclass_constant(base, node.child_by_field_name("object")?),
        _ => None,
    }
}

/// Extract a method symbol
pub(super) fn extract_method(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    scope: &MemberScope,
) -> Option<Symbol> {
    let name = extract_name_from_node(node, |n| base.get_node_text(n), "name")
        .or_else(|| extract_name_from_node(node, |n| base.get_node_text(n), "identifier"))
        .or_else(|| extract_name_from_node(node, |n| base.get_node_text(n), "operator"))
        .or_else(|| {
            // Fallback: find method name by traversing children
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "identifier" | "operator" => {
                        return Some(base.get_node_text(&child));
                    }
                    _ => continue,
                }
            }
            None
        })?;

    let signature = signatures::build_method_signature(&node, &name, |n| base.get_node_text(n));
    let kind = if name == "initialize" {
        SymbolKind::Constructor
    } else {
        SymbolKind::Method
    };

    let doc_comment = find_ruby_doc_comment(base, node);

    let mut metadata = HashMap::new();
    apply_callable_test_metadata(
        "ruby",
        &name,
        &base.file_path,
        &kind,
        &[],
        doc_comment.as_deref(),
        &mut metadata,
    );
    if scope.class_method {
        class_method_metadata(&mut metadata);
    }

    Some(base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(scope.visibility.clone()),
            parent_id,
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract a singleton method symbol: `def self.name`, `def self.name=`,
/// `def self.[]`. A bare `private` does not reach singleton methods; only
/// `private_class_method` does, so they start public.
pub(super) fn extract_singleton_method(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = base.get_node_text(&node.child_by_field_name("name")?);
    let signature =
        signatures::build_singleton_method_signature(&node, &name, |n| base.get_node_text(n));

    let doc_comment = find_ruby_doc_comment(base, node);
    let mut metadata = HashMap::new();
    class_method_metadata(&mut metadata);

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Method,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id,
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract a variable symbol
pub(super) fn extract_variable(base: &mut BaseExtractor, node: Node) -> Symbol {
    let name = base.get_node_text(&node);
    let signature = name.clone();

    let doc_comment = find_ruby_doc_comment(base, node);

    base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: None,
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    )
}

/// Extract a constant symbol
pub(super) fn extract_constant(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Symbol {
    let name = base.get_node_text(&node);
    let signature = name.clone();

    let doc_comment = find_ruby_doc_comment(base, node);

    base.create_symbol(
        &node,
        name,
        SymbolKind::Constant,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id,
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    )
}

/// Extract an alias symbol, a method of the enclosing class or module.
pub(super) fn extract_alias(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    scope: &MemberScope,
) -> Option<Symbol> {
    let signature = base.get_node_text(&node);
    let alias_name = extract_alias_name(node, |n| base.get_node_text(n))?;

    let doc_comment = find_ruby_doc_comment(base, node);

    Some(base.create_symbol(
        &node,
        alias_name,
        SymbolKind::Method,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(scope.visibility.clone()),
            parent_id,
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

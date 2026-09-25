/// Class and type extraction for Python
/// Handles class definitions, enums, protocols, and type detection
use super::super::base::{Symbol, SymbolKind, SymbolOptions, normalize_annotations};
use super::PythonExtractor;
use super::{decorators, helpers};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// Collect the names of every class defined in this file, at any nesting
/// depth, so `x = Foo()` assignments can record an inferred constructed type.
pub(super) fn collect_class_names(extractor: &PythonExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_class_names_into(extractor, root, 0, &mut names);
    names
}

fn collect_class_names_into(
    extractor: &PythonExtractor,
    node: Node,
    depth: u32,
    names: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "class_definition"
        && let Some(name_node) = node.child_by_field_name("name")
    {
        names.insert(extractor.base().get_node_text(&name_node));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_class_names_into(extractor, child, child_depth, names);
    }
}

/// Extract a class definition from a class_definition node
pub(super) fn extract_class(extractor: &mut PythonExtractor, node: Node) -> Option<Symbol> {
    // Use field-based lookup for robustness against grammar changes
    let identifier_node = node.child_by_field_name("name")?;
    let name = extractor.base_mut().get_node_text(&identifier_node);

    // Extract base classes and metaclass arguments
    let superclasses_node = node.child_by_field_name("superclasses");
    let mut extends_info = String::new();
    let mut is_enum = false;
    let mut is_protocol = false;
    let all_args = if let Some(superclasses) = superclasses_node {
        let all_args = helpers::extract_argument_list(extractor, &superclasses);

        // Separate regular base classes from keyword arguments
        let bases: Vec<_> = all_args
            .iter()
            .filter(|arg| !arg.contains('='))
            .cloned()
            .collect();
        let keyword_args: Vec<_> = all_args
            .iter()
            .filter(|arg| arg.contains('='))
            .cloned()
            .collect();

        is_enum = bases.iter().any(|base| is_enum_base(base));
        is_protocol = bases.iter().any(|base| is_protocol_base(base));

        // Build extends information
        let mut extends_parts = Vec::new();
        if !bases.is_empty() {
            extends_parts.push(format!("extends {}", bases.join(", ")));
        }

        // Add metaclass info if present
        if let Some(metaclass_arg) = keyword_args
            .iter()
            .find(|arg| arg.starts_with("metaclass="))
        {
            extends_parts.push(metaclass_arg.clone());
        }

        if !extends_parts.is_empty() {
            extends_info = format!(" {}", extends_parts.join(" "));
        }

        all_args
    } else {
        Vec::new()
    };

    // Extract decorators
    let decorators_list = decorators::extract_decorators(extractor, &node);
    let decorator_texts = decorators::extract_decorator_texts(extractor, &node);
    let annotations = normalize_annotations(&decorator_texts, "python");
    let decorator_info = decorators::signature_prefix(&decorator_texts);

    let type_parameters = node
        .child_by_field_name("type_parameters")
        .map(|type_parameters| extractor.base().get_node_text(&type_parameters))
        .unwrap_or_default();
    let signature = format!(
        "{}class {}{}{}",
        decorator_info, name, type_parameters, extends_info
    );

    // Determine the symbol kind based on base classes
    let symbol_kind = if is_enum {
        SymbolKind::Enum
    } else if is_protocol {
        SymbolKind::Interface
    } else {
        SymbolKind::Class
    };

    // Extract docstring
    let doc_comment = extract_docstring(extractor, &node);

    let parent_id = helpers::find_enclosing_callable_id(extractor, &node);
    let visibility = super::signatures::infer_visibility(&name);

    let mut metadata = HashMap::new();
    metadata.insert("decorators".to_string(), serde_json::json!(decorators_list));
    metadata.insert("superclasses".to_string(), serde_json::json!(all_args));
    metadata.insert("isEnum".to_string(), serde_json::json!(is_enum));

    Some(extractor.base_mut().create_symbol(
        &node,
        name,
        symbol_kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id,
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    ))
}

/// A PEP 695 `type Name[T] = ...` statement as a `type` symbol.
pub(super) fn extract_type_alias(extractor: &mut PythonExtractor, node: Node) -> Option<Symbol> {
    let name_node = type_alias_name_node(node)?;
    let name = extractor.base().get_node_text(&name_node);
    let signature = extractor.base().get_node_text(&node);
    let parent_id = helpers::find_enclosing_callable_id(extractor, &node);
    let visibility = super::signatures::infer_visibility(&name);
    let symbol = extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Type,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id,
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    Some(helpers::without_body(symbol))
}

/// The declared name of a `type_alias_statement`: the identifier of its
/// `left` type, bare or with type parameters (`Pair[T]`).
pub(super) fn type_alias_name_node(node: Node) -> Option<Node> {
    let left = node.child_by_field_name("left")?;
    let head = left.named_child(0)?;
    match head.kind() {
        "identifier" => Some(head),
        "generic_type" => head
            .named_child(0)
            .filter(|name| name.kind() == "identifier"),
        _ => None,
    }
}

/// The PEP 257 docstring of a function or class: the first statement of its
/// body when that statement is a plain string literal. Byte strings and
/// f-strings are not docstrings.
pub(super) fn extract_docstring(extractor: &PythonExtractor, node: &Node) -> Option<String> {
    let body_node = node.child_by_field_name("body")?;
    let mut cursor = body_node.walk();
    let first_statement = body_node
        .named_children(&mut cursor)
        .find(|child| child.kind() != "comment")?;
    string_statement_text(extractor, first_statement)
}

/// The decoded text of an `expression_statement` that holds only a plain
/// string literal.
pub(super) fn string_statement_text(
    extractor: &PythonExtractor,
    statement: Node,
) -> Option<String> {
    if statement.kind() != "expression_statement" || statement.named_child_count() != 1 {
        return None;
    }
    let string = statement.named_child(0)?;
    if string.kind() != "string" {
        return None;
    }
    let text = extractor.base().get_node_text(&string);
    let prefix_len = text.find(['"', '\'']).unwrap_or(0);
    let prefix = text[..prefix_len].to_ascii_lowercase();
    if prefix.contains('f') || prefix.contains('b') {
        return None;
    }
    let docstring = helpers::strip_string_delimiters(&text[prefix_len..]);
    Some(docstring.trim().to_string())
}

/// The class named by `node` subclasses a standard enum base or a Django
/// choices base, bare or module-qualified.
pub(super) fn is_enum_class(extractor: &PythonExtractor, class_node: &Node) -> bool {
    class_node
        .child_by_field_name("superclasses")
        .map(|superclasses| {
            helpers::extract_argument_list(extractor, &superclasses)
                .iter()
                .any(|base| is_enum_base(base))
        })
        .unwrap_or(false)
}

fn is_enum_base(base: &str) -> bool {
    match base.rsplit_once('.') {
        Some(("enum", name)) => ENUM_BASES.contains(&name),
        Some(("models", name)) => DJANGO_CHOICES_BASES.contains(&name),
        Some(_) => false,
        None => ENUM_BASES.contains(&base) || DJANGO_CHOICES_BASES[..2].contains(&base),
    }
}

const ENUM_BASES: [&str; 6] = ["Enum", "IntEnum", "StrEnum", "Flag", "IntFlag", "ReprEnum"];
const DJANGO_CHOICES_BASES: [&str; 3] = ["TextChoices", "IntegerChoices", "Choices"];

fn is_protocol_base(base: &str) -> bool {
    let name = base.split_once('[').map_or(base, |(name, _)| name);
    matches!(
        name,
        "Protocol" | "typing.Protocol" | "typing_extensions.Protocol"
    )
}

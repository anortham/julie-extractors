/// Variable and constant assignment extraction
/// Handles variable assignments, type annotations, enum members, and constants
use super::super::base::{Symbol, SymbolKind, SymbolOptions};
use super::PythonExtractor;
use super::{helpers, signatures, type_facts, types};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// Extract an assignment statement - can return multiple symbols for tuple unpacking
pub(super) fn extract_assignment(extractor: &mut PythonExtractor, node: Node) -> Vec<Symbol> {
    // Handle assignments like: x = 5, x: int = 5, self.x = 5, a, b = 1, 2
    let left = match node.child_by_field_name("left") {
        Some(left) => left,
        None => return vec![],
    };
    let right = node.child_by_field_name("right");

    // Handle multiple assignment patterns (a, b = 1, 2)
    if left.kind() == "pattern_list" || left.kind() == "tuple_pattern" {
        return extract_multiple_assignment_targets(extractor, left, right);
    }

    let (name, mut symbol_kind) = match left.kind() {
        "identifier" => {
            let name = extractor.base_mut().get_node_text(&left);
            (name, SymbolKind::Variable)
        }
        "attribute" => match self_attribute_name(extractor, left) {
            Some(name) => (name, SymbolKind::Property),
            None => return vec![],
        },
        _ => return vec![],
    };
    let is_instance_attribute = left.kind() == "attribute";

    if name == "__slots__" {
        symbol_kind = SymbolKind::Property;
    } else if symbol_kind == SymbolKind::Variable
        && !is_sunder_or_dunder(&name)
        && directly_in_enum_body(extractor, node)
    {
        symbol_kind = SymbolKind::EnumMember;
    } else if symbol_kind == SymbolKind::Variable && name == name.to_uppercase() && name.len() > 1 {
        symbol_kind = SymbolKind::Constant;
    }

    let type_node = signatures::find_type_annotation(&node);
    let type_annotation = if let Some(type_node) = type_node {
        format!(": {}", extractor.base_mut().get_node_text(&type_node))
    } else {
        String::new()
    };

    // Extract value for signature
    let value = if let Some(right) = right {
        extractor.base_mut().get_node_text(&right)
    } else {
        String::new()
    };

    let target = if is_instance_attribute {
        format!("self.{name}")
    } else {
        name.clone()
    };
    let signature = if right.is_some() {
        format!("{target}{type_annotation} = {value}")
    } else {
        format!("{target}{type_annotation}")
    };

    // Infer visibility from name
    let visibility = signatures::infer_visibility(&name);

    let parent_id = if symbol_kind == crate::base::SymbolKind::Property {
        helpers::find_parent_class_id(extractor, &node)
    } else {
        helpers::find_enclosing_callable_id(extractor, &node)
    };

    let mut metadata = HashMap::new();
    metadata.insert(
        "hasTypeAnnotation".to_string(),
        serde_json::json!(!type_annotation.is_empty()),
    );

    let doc_comment = attribute_doc_comment(extractor, node);

    let symbol = extractor.base_mut().create_symbol(
        &node,
        name,
        symbol_kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id,
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    );

    if let Some(type_node) = type_node {
        type_facts::record_annotation_fact(extractor.base_mut(), &symbol.id, type_node);
    } else if let Some(right) = right
        && symbol.kind != SymbolKind::EnumMember
    {
        type_facts::record_initializer_fact(extractor, &symbol.id, right);
    }
    if is_instance_attribute {
        extractor.instance_attribute_ids.insert(symbol.id.clone());
    }

    vec![helpers::without_body(symbol)]
}

fn is_sunder_or_dunder(name: &str) -> bool {
    name.len() > 2 && name.starts_with('_') && name.ends_with('_')
}

/// The assignment is a statement of an enum class body (not of a method).
fn directly_in_enum_body(extractor: &PythonExtractor, assignment: Node) -> bool {
    let class_node = assignment
        .parent()
        .filter(|statement| statement.kind() == "expression_statement")
        .and_then(|statement| statement.parent())
        .filter(|block| block.kind() == "block")
        .and_then(|block| block.parent())
        .filter(|class_node| class_node.kind() == "class_definition");
    class_node.is_some_and(|class_node| types::is_enum_class(extractor, &class_node))
}

/// The documentation of a module or class attribute: the Sphinx `#:` comment
/// lines right above the statement, else the PEP 257 attribute docstring (a
/// string statement right after it).
fn attribute_doc_comment(extractor: &PythonExtractor, assignment: Node) -> Option<String> {
    let statement = assignment
        .parent()
        .filter(|statement| statement.kind() == "expression_statement")?;
    let scope = statement.parent()?;
    let class_body = scope.kind() == "block"
        && scope
            .parent()
            .is_some_and(|owner| owner.kind() == "class_definition");
    if scope.kind() != "module" && !class_body {
        return None;
    }
    let mut lines = Vec::new();
    let mut expected_row = statement.start_position().row;
    let mut previous = statement.prev_sibling().or_else(|| {
        // A comment above the first statement of a class body is a child of
        // the class definition, not of its body block.
        class_body.then(|| scope.prev_sibling()).flatten()
    });
    while let Some(comment) = previous.filter(|node| node.kind() == "comment") {
        let text = extractor.base().get_node_text(&comment);
        let Some(line) = text.strip_prefix("#:") else {
            break;
        };
        if comment.end_position().row + 1 != expected_row {
            break;
        }
        lines.push(
            line.strip_prefix(' ')
                .unwrap_or(line)
                .trim_end()
                .to_string(),
        );
        expected_row = comment.start_position().row;
        previous = comment.prev_sibling();
    }
    if !lines.is_empty() {
        lines.reverse();
        return Some(lines.join("\n"));
    }
    let next = statement.next_named_sibling()?;
    types::string_statement_text(extractor, next)
}

/// The attribute name of a `self.x` assignment target.
fn self_attribute_name(extractor: &PythonExtractor, target: Node) -> Option<String> {
    let object = target.child_by_field_name("object")?;
    let attribute = target.child_by_field_name("attribute")?;
    (extractor.base().get_node_text(&object) == "self")
        .then(|| extractor.base().get_node_text(&attribute))
}

/// Keep one member row per class attribute. A class-level declaration wins;
/// otherwise the first `self.x` assignment in source order does. Later
/// assignments stay visible as member-access identifiers.
pub(super) fn keep_first_attribute_declaration(
    symbols: &mut Vec<Symbol>,
    instance_attribute_ids: &HashSet<String>,
) {
    let mut declared: HashSet<(String, String)> = symbols
        .iter()
        .filter(|symbol| !instance_attribute_ids.contains(&symbol.id))
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Variable
                    | SymbolKind::Constant
                    | SymbolKind::EnumMember
                    | SymbolKind::Property
            )
        })
        .filter_map(|symbol| Some((symbol.parent_id.clone()?, symbol.name.clone())))
        .collect();
    symbols.retain(|symbol| {
        if !instance_attribute_ids.contains(&symbol.id) {
            return true;
        }
        let Some(parent_id) = symbol.parent_id.clone() else {
            return true;
        };
        declared.insert((parent_id, symbol.name.clone()))
    });
}

/// Extract multiple assignment targets from pattern_list or tuple_pattern
/// Example: a, b = 1, 2 extracts both 'a' and 'b' as separate variables
fn extract_multiple_assignment_targets(
    extractor: &mut PythonExtractor,
    left_node: Node,
    right: Option<Node>,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    // Extract the right-hand side value for signature
    let value = if let Some(right) = right {
        extractor.base_mut().get_node_text(&right)
    } else {
        String::new()
    };

    let parent_id = helpers::find_enclosing_callable_id(extractor, &left_node);

    let mut cursor = left_node.walk();
    for child in left_node.children(&mut cursor) {
        let (name, symbol_kind, parent_id) = match child.kind() {
            "identifier" => {
                let name = extractor.base_mut().get_node_text(&child);
                let kind = if name == name.to_uppercase() && name.len() > 1 {
                    SymbolKind::Constant
                } else {
                    SymbolKind::Variable
                };
                (name, kind, parent_id.clone())
            }
            "attribute" => match self_attribute_name(extractor, child) {
                Some(name) => (
                    name,
                    SymbolKind::Property,
                    helpers::find_parent_class_id(extractor, &child),
                ),
                None => continue,
            },
            _ => continue,
        };
        let signature = if symbol_kind == SymbolKind::Property {
            format!("self.{name} = {value}")
        } else {
            format!("{name} = {value}")
        };

        let visibility = signatures::infer_visibility(&name);

        let doc_comment = attribute_doc_comment(extractor, left_node.parent().unwrap_or(left_node));

        let symbol = extractor.base_mut().create_symbol(
            &child,
            name,
            symbol_kind.clone(),
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(visibility),
                parent_id,
                metadata: None,
                doc_comment,
                annotations: Vec::new(),
            },
        );

        if symbol_kind == SymbolKind::Property {
            extractor.instance_attribute_ids.insert(symbol.id.clone());
        }
        symbols.push(helpers::without_body(symbol));
    }

    symbols
}

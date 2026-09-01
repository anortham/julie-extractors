use super::FSharpExtractor;
use crate::base::{
    AnnotationMarker, BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility,
    normalize_annotations,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

const TYPE_BODY_KINDS: &[&str] = &[
    "anon_type_defn",
    "delegate_type_defn",
    "enum_type_defn",
    "interface_type_defn",
    "record_type_defn",
    "type_abbrev_defn",
    "union_type_defn",
];

pub(super) fn extract_symbols(extractor: &mut FSharpExtractor, root: Node) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    extractor.visit_node(root, &mut symbols, None, 0);
    symbols
}

pub(super) fn visit_node(
    extractor: &mut FSharpExtractor,
    node: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let symbol = extract_symbol(extractor.base(), node, parent_id.clone());
    let next_parent_id = symbol
        .as_ref()
        .map(|symbol| symbol.id.clone())
        .or(parent_id);
    if let Some(symbol) = symbol {
        let callable_id = symbol.id.clone();
        symbols.push(symbol);
        symbols.extend(super::parameters::extract_parameter_symbols(
            extractor.base(),
            node,
            &callable_id,
        ));
    }

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

fn extract_symbol(
    extractor: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    match node.kind() {
        "namespace" => extract_namespace(extractor, node, parent_id),
        "named_module" | "module_defn" => extract_module(extractor, node, parent_id),
        "type_definition" => extract_type(extractor, node, parent_id),
        "record_field" | "union_type_field" => extract_field(extractor, node, parent_id),
        "union_type_case" => extract_union_case(extractor, node, parent_id),
        "member_defn" => extract_member(extractor, node, parent_id),
        "member_signature" => extract_member_signature(extractor, node, parent_id),
        "declaration_expression" => extract_declaration_expression(extractor, node, parent_id),
        "value_definition" => extract_value_definition(extractor, node, parent_id),
        "function_or_value_defn"
            if node
                .parent()
                .is_none_or(|parent| parent.kind() != "declaration_expression") =>
        {
            extract_function_or_value(extractor, node, parent_id)
        }
        _ => None,
    }
}

fn extract_namespace(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = node
        .child_by_field_name("name")
        .map(|name| base.get_node_text(&name))?;
    create_symbol(
        base,
        node,
        name.trim().to_string(),
        SymbolKind::Namespace,
        parent_id,
    )
}

fn extract_module(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name_node = node
        .child_by_field_name("name")
        .or_else(|| direct_child_of_kind(node, "identifier"))?;
    let name = base.get_node_text(&name_node).trim().to_string();
    create_symbol(base, node, name, SymbolKind::Module, parent_id)
}

fn extract_type(base: &mut BaseExtractor, node: Node, parent_id: Option<String>) -> Option<Symbol> {
    let body = direct_child_matching(node, TYPE_BODY_KINDS)?;
    let type_name = direct_child_of_kind(body, "type_name")?;
    let name_node = type_name.child_by_field_name("type_name")?;
    let name = base.get_node_text(&name_node).trim().to_string();
    let kind = match body.kind() {
        "record_type_defn" => SymbolKind::Struct,
        "union_type_defn" if is_type_abbreviation(body) => SymbolKind::Type,
        "union_type_defn" => SymbolKind::Union,
        "interface_type_defn" => SymbolKind::Interface,
        "enum_type_defn" => SymbolKind::Enum,
        "delegate_type_defn" => SymbolKind::Delegate,
        "anon_type_defn" => SymbolKind::Class,
        _ => SymbolKind::Type,
    };

    create_symbol(base, node, name, kind, parent_id)
}

fn extract_field(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = first_identifier_text(base, node)?;
    create_symbol(base, node, name, SymbolKind::Field, parent_id)
}

fn extract_union_case(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    if in_type_abbreviation(node) {
        return None;
    }
    let name = direct_child_of_kind(node, "identifier")
        .map(|name| base.get_node_text(&name).trim().to_string())?;
    create_symbol(base, node, name, SymbolKind::EnumMember, parent_id)
}

fn extract_member(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    if let Some(definition) = direct_child_of_kind(node, "method_or_prop_defn") {
        let name_node = definition.child_by_field_name("name")?;
        let name_node = name_node
            .child_by_field_name("method")
            .or_else(|| direct_child_of_kind(name_node, "identifier"))?;
        let name = base.get_node_text(&name_node).trim().to_string();
        let kind = if definition.child_by_field_name("args").is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Property
        };
        return create_symbol_with_context(base, definition, node, name, kind, parent_id);
    }

    None
}

fn extract_member_signature(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = direct_child_of_kind(node, "identifier")
        .map(|name| base.get_node_text(&name).trim().to_string())?;
    let kind = if base.get_node_text(&node).contains("->") {
        SymbolKind::Method
    } else {
        SymbolKind::Property
    };
    let context = node
        .parent()
        .filter(|parent| parent.kind() == "member_defn")
        .unwrap_or(node);
    create_symbol_with_context(base, node, context, name, kind, parent_id)
}

fn extract_declaration_expression(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let definition = direct_child_of_kind(node, "function_or_value_defn")?;
    extract_function_or_value_with_carrier(base, definition, node, parent_id)
}

fn extract_value_definition(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let left = direct_child_of_kind(node, "value_declaration_left")?;
    let name = first_identifier_text(base, left)?;
    let kind = if base.get_node_text(&node).contains("->") {
        SymbolKind::Function
    } else {
        SymbolKind::Variable
    };
    create_symbol(base, node, name, kind, parent_id)
}

fn extract_function_or_value(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    extract_function_or_value_with_carrier(base, node, node, parent_id)
}

fn extract_function_or_value_with_carrier(
    base: &mut BaseExtractor,
    definition: Node,
    carrier: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let (name, kind) =
        if let Some(left) = direct_child_of_kind(definition, "function_declaration_left") {
            (
                direct_child_of_kind(left, "identifier")
                    .map(|name| base.get_node_text(&name).trim().to_string())?,
                SymbolKind::Function,
            )
        } else {
            let left = direct_child_of_kind(definition, "value_declaration_left")?;
            (first_identifier_text(base, left)?, SymbolKind::Variable)
        };
    create_symbol_with_context(base, definition, carrier, name, kind, parent_id)
}

fn create_symbol(
    base: &mut BaseExtractor,
    node: Node,
    name: String,
    kind: SymbolKind,
    parent_id: Option<String>,
) -> Option<Symbol> {
    create_symbol_with_context(base, node, node, name, kind, parent_id)
}

fn create_symbol_with_context(
    base: &mut BaseExtractor,
    node: Node,
    context: Node,
    name: String,
    kind: SymbolKind,
    parent_id: Option<String>,
) -> Option<Symbol> {
    if name.is_empty() {
        return None;
    }

    let signature = signature_for(base, &name, &kind, node);
    let doc_comment = find_doc_comment(base, context);
    let annotations = annotation_markers(base, context);
    Some(base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility_for(base, context)),
            parent_id,
            doc_comment,
            annotations,
            ..Default::default()
        },
    ))
}

fn signature_for(base: &BaseExtractor, name: &str, kind: &SymbolKind, node: Node) -> String {
    let keyword = match kind {
        SymbolKind::Namespace => "namespace",
        SymbolKind::Module => "module",
        SymbolKind::Struct
        | SymbolKind::Union
        | SymbolKind::Interface
        | SymbolKind::Enum
        | SymbolKind::Delegate
        | SymbolKind::Class
        | SymbolKind::Type => "type",
        SymbolKind::Method | SymbolKind::Property => "member",
        SymbolKind::Function | SymbolKind::Variable => "let",
        SymbolKind::Field | SymbolKind::EnumMember => "field",
        _ => "val",
    };
    let source = base.get_node_text(&node);
    let first_line = source
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    if first_line.trim().is_empty() {
        format!("{keyword} {name}")
    } else {
        first_line.trim().to_string()
    }
}

fn visibility_for(base: &BaseExtractor, node: Node) -> Visibility {
    let modifier = direct_child_of_kind(node, "access_modifier")
        .map(|child| base.get_node_text(&child))
        .unwrap_or_default()
        .to_ascii_lowercase();
    match modifier.as_str() {
        "private" => Visibility::Private,
        "internal" => Visibility::Internal,
        "protected" => Visibility::Protected,
        "public" => Visibility::Public,
        _ => Visibility::Public,
    }
}

fn annotation_markers(base: &BaseExtractor, node: Node) -> Vec<AnnotationMarker> {
    let Some(attributes) = direct_child_of_kind(node, "attributes") else {
        return Vec::new();
    };
    let raw_texts: Vec<String> = attributes
        .children(&mut attributes.walk())
        .filter(|child| child.kind() == "attribute")
        .map(|child| base.get_node_text(&child))
        .collect();
    normalize_annotations(&raw_texts, "fsharp")
}

fn find_doc_comment(base: &BaseExtractor, node: Node) -> Option<String> {
    let lines: Vec<&str> = base.content.lines().collect();
    let mut row = node.start_position().row;
    let mut docs = Vec::new();
    while row > 0 {
        let line = lines.get(row - 1)?.trim();
        if let Some(doc) = line.strip_prefix("///") {
            if !doc.trim().is_empty() {
                docs.push(doc.trim().to_string());
            }
            row -= 1;
            continue;
        }
        if line.starts_with("[<") && line.ends_with(">]") {
            row -= 1;
            continue;
        }
        break;
    }
    if docs.is_empty() {
        None
    } else {
        docs.reverse();
        Some(docs.join("\n"))
    }
}

fn first_identifier_text(base: &BaseExtractor, node: Node) -> Option<String> {
    first_identifier_text_at_depth(base, node, 0)
}

fn first_identifier_text_at_depth(base: &BaseExtractor, node: Node, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == "identifier" {
        let text = base.get_node_text(&node).trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "access_modifier" | "attributes" | "xml_doc") {
            continue;
        }
        if let Some(name) = first_identifier_text_at_depth(base, child, child_depth) {
            return Some(name);
        }
    }
    None
}

fn direct_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn direct_child_matching<'a>(node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| kinds.contains(&child.kind()))
}

fn is_type_abbreviation(body: Node) -> bool {
    let mut cases = Vec::new();
    collect_union_cases(body, 0, &mut cases);
    if cases.len() != 1 {
        return false;
    }
    let case = cases[0];
    let mut cursor = case.walk();
    if case
        .children(&mut cursor)
        .any(|child| matches!(child.kind(), ":" | "of" | "union_type_fields"))
    {
        return false;
    }
    if direct_child_of_kind(case, "identifier").is_none() {
        return false;
    }
    !has_case_bar(body)
}

fn has_case_bar(body: Node) -> bool {
    let mut cursor = body.walk();
    body.children(&mut cursor)
        .filter(|child| child.kind() == "union_type_cases")
        .any(|cases| {
            let mut inner = cases.walk();
            cases.children(&mut inner).any(|child| child.kind() == "|")
        })
}

fn in_type_abbreviation(case: Node) -> bool {
    let mut current = case.parent();
    while let Some(node) = current {
        if node.kind() == "union_type_defn" {
            return is_type_abbreviation(node);
        }
        current = node.parent();
    }
    false
}

fn collect_union_cases<'a>(node: Node<'a>, depth: u32, out: &mut Vec<Node<'a>>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "union_type_case" {
        out.push(node);
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_union_cases(child, child_depth, out);
    }
}

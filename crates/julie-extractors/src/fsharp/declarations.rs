use super::FSharpExtractor;
use crate::base::{
    AnnotationMarker, BaseExtractor, BodySpan, NormalizedSpan, Symbol, SymbolKind, SymbolOptions,
    Visibility, normalize_annotations,
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
    if let Some(definition) = binding_group(node) {
        visit_binding_group(extractor, node, definition, symbols, parent_id, depth);
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
        kind if TYPE_BODY_KINDS.contains(&kind) => extract_type(extractor, node, parent_id),
        "exception_definition" => extract_exception(extractor, node, parent_id),
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

/// One symbol per type body, so `type A = ... and B = ...` yields both. The
/// first body spans from the `type` keyword and carries its attributes and
/// doc comment.
fn extract_type(base: &mut BaseExtractor, body: Node, parent_id: Option<String>) -> Option<Symbol> {
    let definition = body
        .parent()
        .filter(|parent| parent.kind() == "type_definition")?;
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

    let is_first = direct_child_matching(definition, TYPE_BODY_KINDS)
        .is_some_and(|first| first.id() == body.id());
    let (context, start) = if is_first {
        (definition, definition)
    } else {
        (
            body,
            body.prev_sibling()
                .filter(|previous| previous.kind() == "and")
                .unwrap_or(body),
        )
    };
    let span = join_spans(start, body);
    create_symbol_with_span(base, body, context, span, name, kind, parent_id)
}

fn extract_exception(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("exception_name")?;
    let name = terminal_identifier_text(base, name_node)?;
    create_symbol(base, node, name, SymbolKind::Class, parent_id)
}

/// Named record and union fields. An unnamed union or exception field
/// (`Cash of decimal`) is only a type, so it yields no symbol.
fn extract_field(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = direct_child_of_kind(node, "identifier")
        .map(|name| base.get_node_text(&name).trim().to_string())?;
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
    create_symbol_with_span(
        base,
        node,
        context,
        NormalizedSpan::from_node(&node),
        name,
        kind,
        parent_id,
    )
}

fn create_symbol_with_span(
    base: &mut BaseExtractor,
    node: Node,
    context: Node,
    span: NormalizedSpan,
    name: String,
    kind: SymbolKind,
    parent_id: Option<String>,
) -> Option<Symbol> {
    if name.is_empty() {
        return None;
    }

    let signature_node = if TYPE_BODY_KINDS.contains(&node.kind()) || is_binding_left(node.kind()) {
        context
    } else {
        node
    };
    let signature = signature_for(base, &name, &kind, signature_node);
    let doc_comment = find_doc_comment(base, context);
    let annotations = annotation_markers(base, context);
    let visibility = visibility_for(base, node, context);
    Some(base.create_symbol_from_span(
        &node,
        span,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
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

/// The access modifier on the declaration, its binding head, or its type
/// name. Without one, a `let` inside a class is private and everything else
/// is public.
fn visibility_for(base: &BaseExtractor, node: Node, context: Node) -> Visibility {
    let modifier = [context, node]
        .into_iter()
        .flat_map(|owner| {
            let mut cursor = owner.walk();
            let children: Vec<Node> = owner.children(&mut cursor).collect();
            children
        })
        .flat_map(|child| {
            if matches!(
                child.kind(),
                "function_declaration_left" | "value_declaration_left" | "type_name"
            ) {
                let mut cursor = child.walk();
                let inner: Vec<Node> = child.children(&mut cursor).collect();
                inner
            } else {
                vec![child]
            }
        })
        .chain(
            matches!(
                node.kind(),
                "function_declaration_left" | "value_declaration_left"
            )
            .then(|| direct_child_of_kind(node, "access_modifier"))
            .flatten(),
        )
        .find(|child| child.kind() == "access_modifier")
        .map(|child| base.get_node_text(&child).to_ascii_lowercase());
    match modifier.as_deref().map(str::trim) {
        Some("private") => Visibility::Private,
        Some("internal") => Visibility::Internal,
        Some("protected") => Visibility::Protected,
        Some("public") => Visibility::Public,
        _ if is_class_let_binding(node) => Visibility::Private,
        _ => Visibility::Public,
    }
}

/// A `let` or `static let` binding inside a class body: always private to
/// the type in F#.
fn is_class_let_binding(node: Node) -> bool {
    if !matches!(
        node.kind(),
        "function_or_value_defn" | "function_declaration_left" | "value_declaration_left"
    ) {
        return false;
    }
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "anon_type_defn" => return true,
            "function_or_value_defn"
            | "member_defn"
            | "module_defn"
            | "named_module"
            | "namespace"
            | "file" => return false,
            _ => current = candidate.parent(),
        }
    }
    false
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

/// The `function_or_value_defn` of a `let rec f ... and g ...` group with
/// two or more bindings, when `node` carries it.
fn binding_group(node: Node) -> Option<Node> {
    let definition = match node.kind() {
        "declaration_expression" => direct_child_of_kind(node, "function_or_value_defn")?,
        "function_or_value_defn"
            if node
                .parent()
                .is_none_or(|parent| parent.kind() != "declaration_expression") =>
        {
            node
        }
        _ => return None,
    };
    let mut cursor = definition.walk();
    let lefts = definition
        .children(&mut cursor)
        .filter(|child| is_binding_left(child.kind()))
        .count();
    (lefts > 1).then_some(definition)
}

fn is_binding_left(kind: &str) -> bool {
    matches!(kind, "function_declaration_left" | "value_declaration_left")
}

/// Emits one symbol per binding of a `let rec ... and ...` group and visits
/// each binding's nodes under its own symbol.
fn visit_binding_group(
    extractor: &mut FSharpExtractor,
    carrier: Node,
    definition: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<String>,
    depth: u32,
) {
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = definition.walk();
    let children: Vec<Node> = definition.children(&mut cursor).collect();
    let mut segments: Vec<Vec<Node>> = Vec::new();
    for child in children {
        if is_binding_left(child.kind()) {
            segments.push(Vec::new());
        }
        if let Some(segment) = segments.last_mut() {
            segment.push(child);
        }
    }
    for (index, segment) in segments.iter().enumerate() {
        let (Some(left), Some(last)) = (segment.first(), segment.last()) else {
            continue;
        };
        let start = if index == 0 {
            carrier
        } else {
            left.prev_sibling()
                .filter(|previous| previous.kind() == "and")
                .unwrap_or(*left)
        };
        let context = if index == 0 { carrier } else { *left };
        let symbol = binding_name(extractor.base(), *left).and_then(|(name, kind)| {
            create_symbol_with_span(
                extractor.base(),
                *left,
                context,
                join_spans(start, *last),
                name,
                kind,
                parent_id.clone(),
            )
        });
        let segment_parent = symbol.as_ref().map(|s| s.id.clone()).or(parent_id.clone());
        if let Some(symbol) = symbol {
            let callable_id = symbol.id.clone();
            symbols.push(symbol);
            symbols.extend(super::parameters::extract_parameter_symbols(
                extractor.base(),
                *left,
                &callable_id,
            ));
        }
        for node in segment {
            visit_node(
                extractor,
                *node,
                symbols,
                segment_parent.clone(),
                child_depth,
            );
        }
    }
}

fn binding_name(base: &BaseExtractor, left: Node) -> Option<(String, SymbolKind)> {
    if left.kind() == "function_declaration_left" {
        let name = direct_child_of_kind(left, "identifier")
            .map(|name| base.get_node_text(&name).trim().to_string())?;
        Some((name, SymbolKind::Function))
    } else {
        Some((first_identifier_text(base, left)?, SymbolKind::Variable))
    }
}

fn join_spans(start: Node, end: Node) -> NormalizedSpan {
    let mut span = NormalizedSpan::from_node(&start);
    let end = NormalizedSpan::from_node(&end);
    span.end_line = end.end_line;
    span.end_column = end.end_column;
    span.end_byte = end.end_byte;
    span
}

fn terminal_identifier_text(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.named_children(&mut cursor).collect();
    let name = if node.kind() == "identifier" {
        Some(node)
    } else {
        children
            .into_iter()
            .rev()
            .find(|child| child.kind() == "identifier")
    }?;
    Some(base.get_node_text(&name).trim().to_string())
}

/// The body span of an F# declaration node: the expression after `=` for
/// bindings and members, the member blocks of a type, and the declarations
/// after a module or namespace header. Signatures, fields, union cases, and
/// parameters have no body.
pub(super) fn body_span(node: &Node, _content: &str) -> Option<BodySpan> {
    let node = *node;
    match node.kind() {
        "function_or_value_defn" => node
            .child_by_field_name("body")
            .map(|body| NormalizedSpan::from_node(&body)),
        "function_declaration_left" | "value_declaration_left" => {
            let definition = node.parent()?;
            let mut cursor = definition.walk();
            let body = definition
                .children_by_field_name("body", &mut cursor)
                .find(|body| body.start_byte() >= node.end_byte())?;
            Some(NormalizedSpan::from_node(&body))
        }
        "method_or_prop_defn" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let start = children
                .iter()
                .position(|child| child.kind() == "=")
                .and_then(|index| children[index + 1..].iter().find(|child| child.is_named()))
                .or_else(|| {
                    children.iter().enumerate().find_map(|(index, child)| {
                        (node.field_name_for_child(index as u32) == Some("block")).then_some(child)
                    })
                })?;
            Some(join_spans(*start, node))
        }
        "namespace" | "named_module" => {
            let name = node.child_by_field_name("name")?;
            let mut cursor = node.walk();
            let first = node
                .named_children(&mut cursor)
                .find(|child| child.start_byte() >= name.end_byte())?;
            Some(join_spans(first, node))
        }
        "module_defn" => block_span(node),
        kind if TYPE_BODY_KINDS.contains(&kind) => block_span(node),
        _ => None,
    }
}

fn block_span(node: Node) -> Option<BodySpan> {
    let mut cursor = node.walk();
    let blocks: Vec<Node> = node.children_by_field_name("block", &mut cursor).collect();
    Some(join_spans(*blocks.first()?, *blocks.last()?))
}

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
    "type_declaration",
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
    if node.kind() == "type_extension" {
        visit_type_extension(extractor, node, symbols, parent_id, depth);
        return;
    }

    let symbol = extract_symbol(extractor.base(), node, parent_id.clone());
    let next_parent_id = symbol
        .as_ref()
        .map(|symbol| symbol.id.clone())
        .or(parent_id.clone());
    if let Some(symbol) = symbol {
        let callable_id = symbol.id.clone();
        let is_value = symbol.kind == SymbolKind::Variable || symbol.kind == SymbolKind::Constant;
        symbols.push(symbol);
        if is_value {
            symbols.extend(extra_pattern_bindings(
                extractor.base(),
                node,
                parent_id.clone(),
            ));
        }
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
    for (index, child) in node.children(&mut cursor).enumerate() {
        // A `let` scopes over the rest of its block (`in`), but the rest of
        // the block belongs to the enclosing declaration, not to the `let`.
        let child_parent = if node.kind() == "declaration_expression"
            && node.field_name_for_child(index as u32) == Some("in")
        {
            parent_id.clone()
        } else {
            next_parent_id.clone()
        };
        visit_node(extractor, child, symbols, child_parent, child_depth);
    }
}

/// `type X with member ...`: the members belong to the same-file type `X`
/// when there is one, else to the enclosing container, and each carries the
/// extended type name as `extendedType`.
fn visit_type_extension(
    extractor: &mut FSharpExtractor,
    node: Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<String>,
    depth: u32,
) {
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let Some(extended) = direct_child_of_kind(node, "type_name")
        .and_then(|type_name| type_name.child_by_field_name("type_name"))
        .map(|name| extractor.base().get_node_text(&name).trim().to_string())
    else {
        return;
    };
    let owner_id = symbols
        .iter()
        .rev()
        .find(|symbol| symbol.name == extended && is_type_kind(&symbol.kind))
        .map(|symbol| symbol.id.clone())
        .or(parent_id);
    let before = symbols.len();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_node(extractor, child, symbols, owner_id.clone(), child_depth);
    }
    for symbol in &mut symbols[before..] {
        if symbol.parent_id == owner_id
            && matches!(
                symbol.kind,
                SymbolKind::Method | SymbolKind::Property | SymbolKind::Event
            )
        {
            symbol.metadata.get_or_insert_with(Default::default).insert(
                "extendedType".to_string(),
                serde_json::Value::String(extended.clone()),
            );
        }
    }
}

fn is_type_kind(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Union
            | SymbolKind::Interface
            | SymbolKind::Enum
            | SymbolKind::Type
            | SymbolKind::Delegate
    )
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
        "enum_type_case" => extract_enum_case(extractor, node, parent_id),
        "member_defn" => extract_member(extractor, node, parent_id),
        "extern_binding" => extract_extern(extractor, node, parent_id),
        "import_decl" => extract_import(extractor, node, parent_id),
        "fsi_directive_decl" => extract_script_directive(extractor, node, parent_id),
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
    let name = namespace_name(node).map(|name| base.get_node_text(&name))?;
    create_symbol(
        base,
        node,
        name.trim().to_string(),
        SymbolKind::Namespace,
        parent_id,
    )
}

/// The dotted name of a namespace. The grammar's `name` field also covers
/// the anonymous `rec` keyword of `namespace rec X`.
fn namespace_name(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("name", &mut cursor)
        .filter(|name| name.is_named())
        .last()
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
    let is_first = direct_child_matching(definition, TYPE_BODY_KINDS)
        .is_some_and(|first| first.id() == body.id());
    let attribute_keys = if is_first {
        attribute_keys(base, definition)
    } else {
        Vec::new()
    };
    let has_attribute = |key: &str| attribute_keys.iter().any(|k| k == key);
    let kind = match body.kind() {
        "record_type_defn" => SymbolKind::Struct,
        "union_type_defn" if is_type_abbreviation(body) => SymbolKind::Type,
        "union_type_defn" => SymbolKind::Union,
        "interface_type_defn" => SymbolKind::Interface,
        "enum_type_defn" => SymbolKind::Enum,
        "delegate_type_defn" => SymbolKind::Delegate,
        "anon_type_defn" if has_attribute("interface") || is_abstract_only(body) => {
            SymbolKind::Interface
        }
        "anon_type_defn" if has_attribute("struct") => SymbolKind::Struct,
        "anon_type_defn" => SymbolKind::Class,
        _ => SymbolKind::Type,
    };

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

/// A type with no primary constructor whose members are all abstract
/// signatures is an interface in F#.
fn is_abstract_only(body: Node) -> bool {
    if direct_child_of_kind(body, "primary_constr_args").is_some() {
        return false;
    }
    let mut cursor = body.walk();
    let members: Vec<Node> = body
        .children_by_field_name("block", &mut cursor)
        .flat_map(|block| {
            let mut block_cursor = block.walk();
            block.named_children(&mut block_cursor).collect::<Vec<_>>()
        })
        .collect();
    !members.is_empty()
        && members.iter().all(|member| {
            member.kind() == "member_defn"
                && direct_child_of_kind(*member, "member_signature").is_some()
        })
}

/// Normalized attribute keys (`struct`, `interface`, `literal`) directly on
/// a declaration.
fn attribute_keys(base: &BaseExtractor, node: Node) -> Vec<String> {
    annotation_markers(base, node)
        .into_iter()
        .map(|marker| marker.annotation_key)
        .collect()
}

fn extract_enum_case(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = direct_child_of_kind(node, "identifier")
        .map(|name| base.get_node_text(&name).trim().to_string())?;
    create_symbol(base, node, name, SymbolKind::EnumMember, parent_id)
}

fn extract_extern(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = node
        .child_by_field_name("name")
        .map(|name| base.get_node_text(&name).trim().to_string())?;
    create_symbol(base, node, name, SymbolKind::Function, parent_id)
}

/// `open A.B` is an import named by its last segment, as in C#.
fn extract_import(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let target = node.named_children(&mut cursor).next()?;
    let path = base.get_node_text(&target).trim().to_string();
    let name = path.rsplit('.').next().unwrap_or(&path).trim().to_string();
    create_symbol(base, node, name, SymbolKind::Import, parent_id)
}

/// `#load "file.fsx"` and `#r "nuget: Package, 1.0"` / `#r "lib.dll"` are
/// script imports named by the loaded file or referenced package.
fn extract_script_directive(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let text = base.get_node_text(&node);
    let directive = text.trim_start().strip_prefix('#')?;
    let directive = directive
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    if !matches!(directive, "load" | "r") {
        return None;
    }
    let string = direct_child_of_kind(node, "string")?;
    let argument = base
        .get_node_text(&string)
        .trim()
        .trim_matches('"')
        .to_string();
    let mut metadata = std::collections::HashMap::from([(
        "directive".to_string(),
        serde_json::Value::String(directive.to_string()),
    )]);
    let name = match argument.strip_prefix("nuget:") {
        Some(package) => {
            let mut parts = package.split(',').map(str::trim);
            let package_name = parts.next().unwrap_or_default().to_string();
            if let Some(version) = parts.next().filter(|version| !version.is_empty()) {
                metadata.insert(
                    "version".to_string(),
                    serde_json::Value::String(version.to_string()),
                );
            }
            metadata.insert(
                "source".to_string(),
                serde_json::Value::String("nuget".to_string()),
            );
            package_name
        }
        None => argument,
    };
    let span = base.span_for_byte_range(node.start_byte(), string.end_byte())?;
    let mut symbol =
        create_symbol_with_span(base, node, node, span, name, SymbolKind::Import, parent_id)?;
    symbol.metadata = Some(metadata);
    Some(symbol)
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

/// Members: methods and properties (`member x.M`), operator members
/// (`static member (+)`), constructors (`new(...) = ...`), auto-properties
/// (`member val X = e`), and explicit fields (`val X: T`).
fn extract_member(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    if let Some(definition) = direct_child_of_kind(node, "method_or_prop_defn") {
        let name_node = definition.child_by_field_name("name")?;
        let name_node = name_node
            .child_by_field_name("method")
            .or_else(|| direct_child_matching(name_node, &["identifier", "op_identifier"]))?;
        let name = base.get_node_text(&name_node).trim().to_string();
        let kind = if definition.child_by_field_name("args").is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Property
        };
        return create_symbol_with_context(base, definition, node, name, kind, parent_id);
    }
    if let Some(constructor) = direct_child_of_kind(node, "additional_constr_defn") {
        return create_symbol_with_context(
            base,
            constructor,
            node,
            "new".to_string(),
            SymbolKind::Constructor,
            parent_id,
        );
    }
    if let Some(name_node) = direct_child_of_kind(node, "property_or_ident") {
        let name = terminal_identifier_text(base, name_node)?;
        return create_symbol(base, node, name, SymbolKind::Property, parent_id);
    }
    let name = direct_child_of_kind(node, "identifier")
        .map(|name| base.get_node_text(&name).trim().to_string())?;
    create_symbol(base, node, name, SymbolKind::Field, parent_id)
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
    let Some(definition) = direct_child_of_kind(node, "function_or_value_defn") else {
        return extract_use_binding(base, node, parent_id);
    };
    extract_function_or_value_with_carrier(base, definition, node, parent_id)
}

/// `use name = expr` binds a disposable local like `let`.
fn extract_use_binding(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let keyword = direct_child_matching(node, &["use", "use!"])?;
    let name_node = direct_child_of_kind(node, "identifier")?;
    let name = base.get_node_text(&name_node).trim().to_string();
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let value = children
        .iter()
        .position(|child| child.kind() == "=")
        .and_then(|index| children.get(index + 1))
        .copied()
        .unwrap_or(name_node);
    let span = join_spans(keyword, value);
    let signature = base
        .get_node_text(&node)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    Some(base.create_symbol_from_span(
        &name_node,
        span,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id,
            ..Default::default()
        },
    ))
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
    let left = direct_child_of_kind(definition, "function_declaration_left")
        .or_else(|| direct_child_of_kind(definition, "value_declaration_left"))?;
    let (name, kind) = binding_name(base, left)?;
    let kind = if kind == SymbolKind::Variable {
        value_kind(base, definition, carrier)
    } else {
        kind
    };
    create_symbol_with_context(base, definition, carrier, name, kind, parent_id)
}

/// A value bound to `function ...` or `fun ... ->` is a function; a
/// `[<Literal>]` value is a constant.
fn value_kind(base: &BaseExtractor, definition: Node, carrier: Node) -> SymbolKind {
    let body_kind = definition
        .child_by_field_name("body")
        .map(|body| body.kind());
    if matches!(body_kind, Some("function_expression" | "fun_expression")) {
        return SymbolKind::Function;
    }
    let is_literal = annotation_markers(base, carrier)
        .iter()
        .any(|marker| marker.annotation_key == "literal");
    if is_literal {
        SymbolKind::Constant
    } else {
        SymbolKind::Variable
    }
}

/// Every name a value pattern binds after the first: `let a, b = ...`,
/// `let (x, y) = ...`, `let { X = px } = ...`. The first name is the
/// binding's own symbol.
fn extra_pattern_bindings(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Vec<Symbol> {
    let definition = match node.kind() {
        "declaration_expression" => direct_child_of_kind(node, "function_or_value_defn"),
        "function_or_value_defn" => Some(node),
        _ => None,
    };
    let Some(left) = definition.and_then(|d| direct_child_of_kind(d, "value_declaration_left"))
    else {
        return Vec::new();
    };
    let mut patterns = Vec::new();
    collect_bound_patterns(left, 0, &mut patterns);
    let visibility_node = node;
    patterns
        .into_iter()
        .skip(1)
        .filter_map(|pattern| {
            let name = terminal_identifier_text(base, pattern)?;
            let visibility = visibility_for(base, visibility_node, visibility_node);
            let signature = format!("let {}", base.get_node_text(&pattern).trim());
            (!name.is_empty()).then(|| {
                base.create_symbol(
                    &pattern,
                    name,
                    SymbolKind::Variable,
                    SymbolOptions {
                        signature: Some(signature),
                        visibility: Some(visibility),
                        parent_id: parent_id.clone(),
                        ..Default::default()
                    },
                )
            })
        })
        .collect()
}

/// `identifier_pattern` nodes that bind one plain name, in source order.
fn collect_bound_patterns<'a>(node: Node<'a>, depth: u32, out: &mut Vec<Node<'a>>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "identifier_pattern" {
        if node.named_child_count() == 1 {
            out.push(node);
        }
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "attributes" {
            collect_bound_patterns(child, child_depth, out);
        }
    }
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
    let source = match direct_child_of_kind(node, "attributes") {
        Some(attributes) => base
            .content
            .get(attributes.end_byte()..node.end_byte())
            .unwrap_or_default()
            .to_string(),
        None => base.get_node_text(&node),
    };
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
        _ if is_class_let_binding(node) || is_local_binding(node) => Visibility::Private,
        _ => Visibility::Public,
    }
}

/// A `let` inside a function, member, or constructor body: a local, never
/// visible outside it.
fn is_local_binding(node: Node) -> bool {
    if node.kind() != "function_or_value_defn" {
        return false;
    }
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "function_or_value_defn" | "member_defn" | "fun_expression" => return true,
            "anon_type_defn" | "module_defn" | "named_module" | "namespace" | "file" => {
                return false;
            }
            _ => current = candidate.parent(),
        }
    }
    false
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

/// Attributes on a declaration: its own `[<...>]` list, and for a binding
/// the inline form `let [<Literal>] X = ...` inside its head.
fn annotation_markers(base: &BaseExtractor, node: Node) -> Vec<AnnotationMarker> {
    let definition = if node.kind() == "function_or_value_defn" {
        Some(node)
    } else {
        direct_child_of_kind(node, "function_or_value_defn")
    };
    let inline = definition
        .and_then(|definition| {
            direct_child_matching(
                definition,
                &["value_declaration_left", "function_declaration_left"],
            )
        })
        .and_then(|left| direct_child_of_kind(left, "attribute_pattern"))
        .and_then(|pattern| direct_child_of_kind(pattern, "attributes"));
    let raw_texts: Vec<String> = [direct_child_of_kind(node, "attributes"), inline]
        .into_iter()
        .flatten()
        .flat_map(|attributes| {
            let mut cursor = attributes.walk();
            attributes
                .children(&mut cursor)
                .filter(|child| child.kind() == "attribute")
                .collect::<Vec<_>>()
        })
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

/// The name a binding head declares: an identifier, an operator
/// (`(+.)`), or an active pattern (`(|Even|Odd|)`); for a value pattern, the
/// first bound name.
fn binding_name(base: &BaseExtractor, left: Node) -> Option<(String, SymbolKind)> {
    if left.kind() == "function_declaration_left" {
        let name = direct_child_matching(left, &["identifier", "op_identifier", "active_pattern"])
            .map(|name| base.get_node_text(&name).trim().to_string())?;
        Some((name, SymbolKind::Function))
    } else {
        let mut patterns = Vec::new();
        collect_bound_patterns(left, 0, &mut patterns);
        let name = match patterns.first() {
            Some(pattern) => terminal_identifier_text(base, *pattern)?,
            None => first_identifier_text(base, left)?,
        };
        Some((name, SymbolKind::Variable))
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
    let name = terminal_identifier(node)?;
    Some(base.get_node_text(&name).trim().to_string())
}

fn terminal_identifier(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.named_children(&mut cursor).collect();
    children.into_iter().rev().find_map(terminal_identifier)
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
        "member_defn" | "additional_constr_defn" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let body = children
                .iter()
                .position(|child| child.kind() == "=")
                .and_then(|index| children[index + 1..].iter().find(|child| child.is_named()))?;
            Some(join_spans(*body, node))
        }
        "namespace" | "named_module" => {
            let name = namespace_name(node)?;
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

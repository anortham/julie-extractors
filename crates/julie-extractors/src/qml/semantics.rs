use super::QmlExtractor;
use crate::base::{BaseExtractor, Symbol, SymbolKind, UnresolvedTarget, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// The comment above a declaration documents it; an annotated declaration
/// takes the comment above its annotations.
pub(super) fn extract_qml_doc_comment(extractor: &QmlExtractor, node: &Node) -> Option<String> {
    let anchor = node
        .parent()
        .filter(|parent| {
            matches!(
                parent.kind(),
                "ui_annotated_object" | "ui_annotated_object_member"
            )
        })
        .unwrap_or(*node);
    extractor.base.extract_documentation(&anchor).or_else(|| {
        let mut comments = Vec::new();
        let mut current = anchor.prev_named_sibling();
        while let Some(sibling) = current {
            if sibling.kind().contains("comment") {
                let text = extractor.base.get_node_text(&sibling);
                if is_qml_doc_comment(text.trim_start()) {
                    comments.push(text);
                }
                current = sibling.prev_named_sibling();
            } else {
                break;
            }
        }
        comments.reverse();
        if comments.is_empty() {
            None
        } else {
            Some(comments.join("\n"))
        }
    })
}

pub(super) fn infer_visibility(name: &str, force_private: bool) -> Visibility {
    if force_private || name.starts_with('_') {
        Visibility::Private
    } else {
        Visibility::Public
    }
}

pub(super) fn is_signal_handler_binding_name(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix("on") {
        return rest.chars().next().is_some_and(char::is_uppercase);
    }
    if let Some((_, suffix)) = name.rsplit_once(".on") {
        return suffix.chars().next().is_some_and(char::is_uppercase);
    }
    false
}

pub(super) fn handled_signal_from_binding_name(name: &str) -> Option<String> {
    if let Some((_, suffix)) = name.rsplit_once(".on") {
        return lowercase_first(suffix);
    }
    name.strip_prefix("on").and_then(lowercase_first)
}

pub(super) fn property_signature(base: &BaseExtractor, node: Node) -> String {
    let text = base.get_node_text(&node);
    let Some(value) = node.child_by_field_name("value") else {
        return text;
    };
    if value.start_position().row == value.end_position().row {
        return text;
    }
    let head = value.start_byte().saturating_sub(node.start_byte());
    text.get(..head)
        .unwrap_or(text.as_str())
        .trim_end()
        .trim_end_matches(':')
        .trim_end()
        .to_string()
}

/// The `required`, `readonly` and `default` keywords before `property`.
pub(super) fn property_modifiers(base: &BaseExtractor, node: Node) -> Vec<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "ui_property_modifier")
        .map(|modifier| base.get_node_text(&modifier))
        .collect()
}

/// The expression inside an `expression_statement` wrapper, or the node itself.
pub(super) fn unwrap_expression_statement(node: Node) -> Node {
    if node.kind() == "expression_statement" {
        node.named_child(0).unwrap_or(node)
    } else {
        node
    }
}

/// A property's body is its initializer; a literal initializer has no body.
pub(super) fn property_body(value: Node) -> Option<Node> {
    let value = unwrap_expression_statement(value);
    (!matches!(
        value.kind(),
        "number" | "string" | "true" | "false" | "null" | "undefined"
    ))
    .then_some(value)
}

/// A handler's header: `onPressed: (mouse) =>` for a function value, the
/// binding name otherwise.
pub(super) fn handler_signature(base: &BaseExtractor, node: Node, name: &str) -> String {
    let function_body = node
        .child_by_field_name("value")
        .map(unwrap_expression_statement)
        .filter(|value| {
            matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "function"
            )
        })
        .and_then(|function| function.child_by_field_name("body"));
    match function_body {
        Some(body) => base
            .content
            .get(node.start_byte()..body.start_byte())
            .map(|header| header.trim_end().to_string())
            .unwrap_or_else(|| name.to_string()),
        None => name.to_string(),
    }
}

/// The property an object value is bound to: `background` in
/// `background: Rectangle {}` or `wifi` in `property QtObject wifi: QtObject {}`.
pub(super) fn bound_property(base: &BaseExtractor, object: Node) -> Option<String> {
    let parent = object.parent()?;
    if !matches!(parent.kind(), "ui_binding" | "ui_property") {
        return None;
    }
    parent
        .child_by_field_name("value")
        .filter(|value| value.id() == object.id())?;
    parent
        .child_by_field_name("name")
        .map(|name| base.get_node_text(&name))
}

/// Maps each handler that `<receiver>.<signal>.connect(handler)` names to its signal.
pub(super) fn connected_handlers(base: &BaseExtractor, root: Node) -> HashMap<String, String> {
    let mut handlers = HashMap::new();
    collect_connected_handlers(base, root, &mut handlers, 0);
    handlers
}

fn collect_connected_handlers(
    base: &BaseExtractor,
    node: Node,
    handlers: &mut HashMap<String, String>,
    depth: u32,
) {
    if !crate::tree_traversal::should_visit_tree_depth(depth) {
        return;
    }
    if let Some((signal, handler)) = connect_call(base, node) {
        handlers.entry(handler).or_insert(signal);
    }
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_connected_handlers(base, child, handlers, child_depth);
    }
}

/// `(signal, handler)` for a `signal.connect(handler)` call with a named handler.
fn connect_call(base: &BaseExtractor, node: Node) -> Option<(String, String)> {
    if node.kind() != "call_expression" {
        return None;
    }
    let signal = connected_signal_node(base, node.child_by_field_name("function")?)?;
    let arguments = node.child_by_field_name("arguments")?;
    let handler = arguments.named_child(0)?;
    (handler.kind() == "identifier")
        .then(|| (base.get_node_text(&signal), base.get_node_text(&handler)))
}

/// The signal name node of a `<receiver>.<signal>.connect` callee.
pub(super) fn connected_signal_node<'a>(
    base: &BaseExtractor,
    callee: Node<'a>,
) -> Option<Node<'a>> {
    if callee.kind() != "member_expression" {
        return None;
    }
    let property = callee.child_by_field_name("property")?;
    if base.get_node_text(&property) != "connect" {
        return None;
    }
    let object = callee.child_by_field_name("object")?;
    match object.kind() {
        "member_expression" => object.child_by_field_name("property"),
        "identifier" => Some(object),
        _ => None,
    }
}

pub(super) fn signal_parameters(base: &BaseExtractor, node: Node) -> Vec<serde_json::Value> {
    let Some(parameters) = node.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let mut cursor = parameters.walk();
    parameters
        .named_children(&mut cursor)
        .filter(|parameter| parameter.kind() == "ui_signal_parameter")
        .map(|parameter| {
            let mut entry = serde_json::Map::new();
            if let Some(name) = parameter.child_by_field_name("name") {
                entry.insert(
                    "name".to_string(),
                    serde_json::Value::String(base.get_node_text(&name)),
                );
            }
            if let Some(parameter_type) = parameter.child_by_field_name("type") {
                entry.insert(
                    "type".to_string(),
                    serde_json::Value::String(base.get_node_text(&parameter_type)),
                );
            }
            serde_json::Value::Object(entry)
        })
        .collect()
}

pub(super) fn file_declares_singleton(base: &BaseExtractor, root_object: Node) -> bool {
    let mut program = root_object;
    while let Some(parent) = program.parent() {
        program = parent;
    }
    let mut cursor = program.walk();
    program.named_children(&mut cursor).any(|child| {
        child.kind() == "ui_pragma"
            && child
                .child_by_field_name("name")
                .is_some_and(|name| base.get_node_text(&name) == "Singleton")
    })
}

pub(super) fn function_signature(node_text: String) -> String {
    node_text
        .split('{')
        .next()
        .unwrap_or(node_text.as_str())
        .trim()
        .to_string()
}

pub(super) fn infer_types(symbols: &[Symbol]) -> HashMap<String, String> {
    let mut types = HashMap::new();

    for symbol in symbols {
        let inferred = match symbol.kind {
            SymbolKind::Property => symbol
                .signature
                .as_deref()
                .and_then(infer_property_type_from_signature),
            _ => None,
        };

        if let Some(inferred_type) = inferred {
            types.insert(symbol.id.clone(), inferred_type);
        }
    }

    types
}

/// The nearest QML object enclosing `node`: an object definition or a
/// property-value-source binding (`Behavior on color { ... }`).
pub(super) fn enclosing_object(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if matches!(
            parent.kind(),
            "ui_object_definition" | "ui_object_definition_binding"
        ) {
            return Some(parent);
        }
        current = parent;
    }
    None
}

/// Returns `true` for an object that already owns a `Class` row: the file root
/// and an inline component's body. Every other object gets a field row.
pub(super) fn object_has_class_row(object: Node<'_>) -> bool {
    object
        .parent()
        .is_some_and(|parent| parent.kind() == "ui_inline_component")
        || enclosing_object(object).is_none()
}

/// `anchors { fill: parent }` binds a group of properties; only its inner
/// bindings are facts. A lowercase first letter on the terminal type segment
/// tells it apart from an object instantiation.
pub(super) fn is_grouped_property_block(base: &BaseExtractor, node: Node<'_>) -> bool {
    let Some(type_name) = node.child_by_field_name("type_name") else {
        return false;
    };
    let text = base.get_node_text(&type_name);
    let terminal = text.rsplit('.').next().unwrap_or(text.as_str());
    terminal.starts_with(|character: char| character.is_ascii_lowercase())
}

/// The dotted segments of a QML name in source order: `Kirigami.FormData.label`
/// yields three identifier nodes.
pub(super) fn dotted_segments(node: Node<'_>) -> Vec<Node<'_>> {
    if node.kind() != "nested_identifier" {
        return vec![node];
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .flat_map(dotted_segments)
        .collect()
}

/// The attached type a binding name names, as (segment node, name, receiver):
/// `Layout.fillWidth` gives `Layout`, `Kirigami.FormData.label` gives `FormData`
/// with receiver `Kirigami`. A lowercase head (`anchors.fill`) gives nothing.
pub(super) fn attached_type_segment<'a>(
    base: &BaseExtractor,
    name_node: Node<'a>,
) -> Option<(Node<'a>, String, Option<String>)> {
    let segments = dotted_segments(name_node);
    let texts: Vec<String> = segments
        .iter()
        .map(|segment| base.get_node_text(segment))
        .collect();
    let (_, qualifiers) = texts.split_last()?;
    if qualifiers.is_empty() || !starts_uppercase(&texts[0]) {
        return None;
    }
    let index = qualifiers.iter().rposition(|text| starts_uppercase(text))?;
    let receiver = (index > 0).then(|| texts[..index].join("."));
    Some((segments[index], texts[index].clone(), receiver))
}

/// The member a handler name points at, with `true` when it is a property
/// change handler: `onClicked` gives `clicked`, `onColorChanged` gives `color`.
pub(super) fn handler_target_member(name: &str) -> Option<(String, bool)> {
    if !is_signal_handler_binding_name(name) {
        return None;
    }
    let signal = handled_signal_from_binding_name(name)?;
    match signal.strip_suffix("Changed") {
        Some(property) if !property.is_empty() => Some((property.to_string(), true)),
        _ => Some((signal, false)),
    }
}

/// The owner a signal handler points at: a `Connections` object's `target` id,
/// otherwise the enclosing object's type name.
pub(super) fn handler_receiver(base: &BaseExtractor, node: Node) -> Option<String> {
    let object = enclosing_object(node)?;
    if encloses_connections_object(base, node) {
        return connections_target_id(base, object);
    }
    object
        .child_by_field_name("type_name")
        .map(|type_name| base.get_node_text(&type_name))
}

/// `Connections` keeps its handler semantics under an import alias, so
/// `Qml.Connections` counts by its last dotted segment.
pub(super) fn encloses_connections_object(base: &BaseExtractor, node: Node) -> bool {
    enclosing_object_type(base, node)
        .is_some_and(|type_name| type_name.rsplit('.').next() == Some("Connections"))
}

pub(super) fn enclosing_object_type(base: &BaseExtractor, node: Node) -> Option<String> {
    enclosing_object(node)?
        .child_by_field_name("type_name")
        .map(|type_name| base.get_node_text(&type_name))
}

fn connections_target_id(base: &BaseExtractor, object: Node) -> Option<String> {
    let initializer = object.child_by_field_name("initializer")?;
    let mut cursor = initializer.walk();
    let target = initializer.named_children(&mut cursor).find(|child| {
        child.kind() == "ui_binding"
            && child
                .child_by_field_name("name")
                .is_some_and(|name| base.get_node_text(&name) == "target")
    })?;
    let value = target.child_by_field_name("value")?;
    let named = if value.kind() == "expression_statement" {
        value.named_child(0)?
    } else {
        value
    };
    (named.kind() == "identifier").then(|| base.get_node_text(&named))
}

fn starts_uppercase(text: &str) -> bool {
    text.chars().next().is_some_and(char::is_uppercase)
}

pub(super) fn build_unresolved_target(
    base: &BaseExtractor,
    function_node: Node,
    fallback_name: &str,
) -> UnresolvedTarget {
    if function_node.kind() == "member_expression" {
        let receiver = function_node
            .child_by_field_name("object")
            .map(|node| base.get_node_text(&node));
        let property = function_node
            .child_by_field_name("property")
            .map(|node| base.get_node_text(&node))
            .unwrap_or_else(|| fallback_name.to_string());
        let display_name = receiver
            .as_ref()
            .map(|receiver| format!("{receiver}.{property}"))
            .unwrap_or_else(|| property.clone());
        return UnresolvedTarget {
            display_name,
            terminal_name: property,
            receiver,
            namespace_path: Vec::new(),
            import_context: None,
        };
    }

    UnresolvedTarget::simple(fallback_name.to_string())
}

/// `/** */` and `///` are the JSDoc forms; `/*! */` is QDoc, Qt's own format.
fn is_qml_doc_comment(trimmed: &str) -> bool {
    trimmed.starts_with("/**") || trimmed.starts_with("///") || trimmed.starts_with("/*!")
}

fn lowercase_first(value: &str) -> Option<String> {
    let mut chars = value.chars();
    let first = chars.next()?;
    Some(format!("{}{}", first.to_lowercase(), chars.as_str()))
}

fn infer_property_type_from_signature(signature: &str) -> Option<String> {
    let tokens = signature.split_whitespace().collect::<Vec<_>>();
    let property_idx = tokens.iter().position(|token| *token == "property")?;
    let property_type = *tokens.get(property_idx + 1)?;

    if property_type == "alias" {
        return None;
    }

    Some(property_type.trim_end_matches(':').to_string())
}

/// The component a `.qml` file defines: its file name without the extension.
/// A Qt Quick UI Form (`Screen01.ui.qml`) defines `Screen01`.
pub(super) fn component_name(file_path: &str) -> Option<String> {
    let stem = std::path::Path::new(file_path).file_stem()?.to_str()?;
    Some(stem.strip_suffix(".ui").unwrap_or(stem).to_string())
}

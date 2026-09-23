//! `<script setup>` compiler macros that declare component members:
//! `defineProps`, `withDefaults`, `defineEmits`, and `defineModel`.

use super::parsing::VueSection;
use super::script::{call_callee, upsert_member};
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// Declared member types found on the way, as `(symbol id, type)`.
pub(super) type MemberTypes = Vec<(String, String)>;

pub(super) fn declare_macro_members(
    base: &BaseExtractor,
    root: Node<'_>,
    section: &VueSection,
    symbols: &mut Vec<Symbol>,
    types: &mut MemberTypes,
) {
    let source = section.content.as_str();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        let declaration = match statement.kind() {
            "export_statement" => statement.child_by_field_name("declaration"),
            _ => Some(statement),
        };
        let Some(declaration) = declaration else {
            continue;
        };
        match declaration.kind() {
            "lexical_declaration" | "variable_declaration" => {
                let mut declarators = declaration.walk();
                for declarator in declaration.named_children(&mut declarators) {
                    let (Some(name), Some(value)) = (
                        declarator.child_by_field_name("name"),
                        declarator.child_by_field_name("value"),
                    ) else {
                        continue;
                    };
                    let Some(call) = unwrap_call(value) else {
                        continue;
                    };
                    let name_start = (section.content_start + name.start_byte()) as u32;
                    let owner = symbols
                        .iter()
                        .find(|symbol| {
                            symbol.parent_id.is_none()
                                && symbol.start_byte <= name_start
                                && name_start < symbol.end_byte
                                && Some(symbol.name.as_str()) == source.get(name.byte_range())
                        })
                        .map(|symbol| symbol.id.clone());
                    if let Some(owner) = owner {
                        declare_members(base, root, section, call, &owner, symbols, types);
                    }
                }
            }
            "expression_statement" => {
                let Some(call) = declaration.named_child(0).and_then(unwrap_call) else {
                    continue;
                };
                let call_start = (section.content_start + call.start_byte()) as u32;
                let owner = symbols
                    .iter()
                    .find(|symbol| {
                        symbol.start_byte == call_start && symbol.kind == SymbolKind::Function
                    })
                    .map(|symbol| symbol.id.clone());
                if let Some(owner) = owner {
                    declare_members(base, root, section, call, &owner, symbols, types);
                }
            }
            _ => {}
        }
    }
}

fn first_argument(call: Node<'_>) -> Option<Node<'_>> {
    call.child_by_field_name("arguments")?.named_child(0)
}

fn unwrap_call(value: Node<'_>) -> Option<Node<'_>> {
    let value = if value.kind() == "await_expression" {
        value.named_child(0)?
    } else {
        value
    };
    (value.kind() == "call_expression").then_some(value)
}

#[allow(clippy::too_many_arguments)]
fn declare_members(
    base: &BaseExtractor,
    root: Node<'_>,
    section: &VueSection,
    call: Node<'_>,
    owner: &str,
    symbols: &mut Vec<Symbol>,
    types: &mut MemberTypes,
) {
    let source = section.content.as_str();
    let Some(mut callee) = call_callee(call, source) else {
        return;
    };
    let mut call = call;
    if callee == "withDefaults" {
        let Some(inner) = first_argument(call).and_then(unwrap_call) else {
            return;
        };
        let Some(inner_callee) = call_callee(inner, source) else {
            return;
        };
        (call, callee) = (inner, inner_callee);
    }
    let arguments: Vec<Node<'_>> = call
        .child_by_field_name("arguments")
        .map(|arguments| {
            let mut cursor = arguments.walk();
            arguments.named_children(&mut cursor).collect()
        })
        .unwrap_or_default();
    let mut member =
        |node: Node<'_>, name: &str, kind: SymbolKind, group: &str, ty: Option<String>| {
            if let Some(id) =
                upsert_member(base, section, node, name, kind, Some(owner), group, symbols)
                && let Some(ty) = ty
            {
                types.push((id, ty));
            }
        };
    match callee.as_str() {
        "defineProps" => {
            for signature in type_argument_members(root, call, source) {
                if signature.kind() == "property_signature"
                    && let Some(name) = property_name(signature, source)
                {
                    let ty = signature
                        .child_by_field_name("type")
                        .and_then(|annotation| source.get(annotation.byte_range()))
                        .map(|text| text.trim_start_matches(':').trim().to_string());
                    member(signature, &name, SymbolKind::Property, "props", ty);
                }
            }
            match arguments.first() {
                Some(object) if object.kind() == "object" => {
                    for (entry, name) in object_entries(*object, source) {
                        let ty = entry
                            .child_by_field_name("value")
                            .and_then(|value| prop_type(value, source));
                        member(entry, &name, SymbolKind::Property, "props", ty);
                    }
                }
                Some(array) if array.kind() == "array" => {
                    for (entry, name) in string_entries(*array, source) {
                        member(entry, &name, SymbolKind::Property, "props", None);
                    }
                }
                _ => {}
            }
        }
        "defineEmits" => {
            for signature in type_argument_members(root, call, source) {
                let name = match signature.kind() {
                    "call_signature" => event_literal(signature, source),
                    "property_signature" => property_name(signature, source),
                    _ => None,
                };
                if let Some(name) = name {
                    member(signature, &name, SymbolKind::Event, "emits", None);
                }
            }
            match arguments.first() {
                Some(array) if array.kind() == "array" => {
                    for (entry, name) in string_entries(*array, source) {
                        member(entry, &name, SymbolKind::Event, "emits", None);
                    }
                }
                Some(object) if object.kind() == "object" => {
                    for (entry, name) in object_entries(*object, source) {
                        member(entry, &name, SymbolKind::Event, "emits", None);
                    }
                }
                _ => {}
            }
        }
        "defineModel" => {
            let named = arguments
                .first()
                .filter(|argument| argument.kind() == "string")
                .and_then(|argument| string_value(*argument, source).map(|name| (*argument, name)));
            let options = arguments
                .iter()
                .find(|argument| argument.kind() == "object")
                .and_then(|options| prop_type(*options, source));
            let (node, name) = named.unwrap_or((call, "modelValue".to_string()));
            member(node, &name, SymbolKind::Property, "model", options);
            member(
                node,
                &format!("update:{name}"),
                SymbolKind::Event,
                "model",
                None,
            );
        }
        _ => {}
    }
}

/// Members of the macro's type argument: an inline object type, or the
/// interface or type alias of that name declared in the same block.
fn type_argument_members<'t>(root: Node<'t>, call: Node<'t>, source: &str) -> Vec<Node<'t>> {
    let Some(argument) = call
        .child_by_field_name("type_arguments")
        .and_then(|arguments| arguments.named_child(0))
    else {
        return Vec::new();
    };
    let body = match argument.kind() {
        "object_type" => Some(argument),
        "type_identifier" => source
            .get(argument.byte_range())
            .and_then(|name| declared_object_type(root, name, source)),
        _ => None,
    };
    body.map(|body| {
        let mut cursor = body.walk();
        body.named_children(&mut cursor).collect()
    })
    .unwrap_or_default()
}

fn declared_object_type<'t>(root: Node<'t>, name: &str, source: &str) -> Option<Node<'t>> {
    let mut cursor = root.walk();
    root.named_children(&mut cursor).find_map(|statement| {
        let declaration = if statement.kind() == "export_statement" {
            statement.child_by_field_name("declaration")?
        } else {
            statement
        };
        let declared = declaration.child_by_field_name("name")?;
        if source.get(declared.byte_range()) != Some(name) {
            return None;
        }
        match declaration.kind() {
            "interface_declaration" => declaration.child_by_field_name("body"),
            "type_alias_declaration" => declaration
                .child_by_field_name("value")
                .filter(|value| value.kind() == "object_type"),
            _ => None,
        }
    })
}

fn property_name(signature: Node<'_>, source: &str) -> Option<String> {
    let name = signature.child_by_field_name("name")?;
    let text = source.get(name.byte_range())?.trim_matches(['"', '\'']);
    (!text.is_empty()).then(|| text.to_string())
}

/// `(e: 'select', id: number): void` declares the event `select`.
fn event_literal(signature: Node<'_>, source: &str) -> Option<String> {
    let parameters = signature.child_by_field_name("parameters")?;
    let first = parameters.named_child(0)?;
    let annotation = first.child_by_field_name("type")?;
    let text = source.get(annotation.byte_range())?;
    let value = text.trim_start_matches(':').trim();
    let unquoted = value.trim_matches(['"', '\'']);
    (unquoted.len() + 2 == value.len() && !unquoted.is_empty()).then(|| unquoted.to_string())
}

fn object_entries<'t>(object: Node<'t>, source: &str) -> Vec<(Node<'t>, String)> {
    let mut cursor = object.walk();
    object
        .named_children(&mut cursor)
        .filter_map(|entry| {
            let key = match entry.kind() {
                "pair" => entry.child_by_field_name("key")?,
                "method_definition" => entry.child_by_field_name("name")?,
                "shorthand_property_identifier" => entry,
                _ => return None,
            };
            let name = source.get(key.byte_range())?.trim_matches(['"', '\'']);
            (!name.is_empty()).then(|| (entry, name.to_string()))
        })
        .collect()
}

fn string_entries<'t>(array: Node<'t>, source: &str) -> Vec<(Node<'t>, String)> {
    let mut cursor = array.walk();
    array
        .named_children(&mut cursor)
        .filter(|entry| entry.kind() == "string")
        .filter_map(|entry| string_value(entry, source).map(|name| (entry, name)))
        .collect()
}

fn string_value(node: Node<'_>, source: &str) -> Option<String> {
    let text = source
        .get(node.byte_range())?
        .trim_matches(['"', '\'', '`']);
    (!text.is_empty()).then(|| text.to_string())
}

/// The TypeScript type a runtime prop declaration names: `String`,
/// `[String, Number]`, or `{ type: Number }`.
pub(super) fn prop_type(value: Node<'_>, source: &str) -> Option<String> {
    prop_type_at(value, source, 0)
}

fn prop_type_at(value: Node<'_>, source: &str, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let depth = child_tree_depth(depth)?;
    match value.kind() {
        "identifier" => source.get(value.byte_range()).map(constructor_type),
        "array" => {
            let mut cursor = value.walk();
            let parts: Vec<String> = value
                .named_children(&mut cursor)
                .filter_map(|part| prop_type_at(part, source, depth))
                .collect();
            (!parts.is_empty()).then(|| parts.join(" | "))
        }
        "object" => {
            let mut cursor = value.walk();
            value.named_children(&mut cursor).find_map(|pair| {
                let key = pair.child_by_field_name("key")?;
                (pair.kind() == "pair" && source.get(key.byte_range())? == "type")
                    .then(|| pair.child_by_field_name("value"))
                    .flatten()
                    .and_then(|inner| prop_type_at(inner, source, depth))
            })
        }
        "as_expression" | "satisfies_expression" => {
            let written = value.named_child(1)?;
            let text = source.get(written.byte_range())?.trim();
            Some(
                text.strip_prefix("PropType<")
                    .and_then(|inner| inner.strip_suffix('>'))
                    .unwrap_or(text)
                    .to_string(),
            )
        }
        _ => None,
    }
}

fn constructor_type(name: &str) -> String {
    match name {
        "String" => "string",
        "Number" => "number",
        "Boolean" => "boolean",
        "BigInt" => "bigint",
        "Symbol" => "symbol",
        other => other,
    }
    .to_string()
}

use crate::base::{RelationshipKind, Symbol, SymbolKind, SymbolOptions, UnresolvedTarget};
use crate::r::RExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::text_args::{
    argument_list_text, clean_r_name, function_signature, split_top_level_arguments,
};

/// The bound name of an assignment target: an identifier, a backticked name, or a string.
/// Member (`x$y`), slot (`x@s`), and subset (`x[[k]]`) targets write into an
/// existing object and bind no new name.
pub(super) fn assignment_name(extractor: &RExtractor, left: Node) -> Option<String> {
    match left.kind() {
        "identifier" | "string" | "string_content" => {
            clean_r_name(&extractor.base.get_node_text(&left))
        }
        _ => None,
    }
}

/// `env$fn <- function(...)`: the receiver text and the member name.
pub(super) fn member_function_target(
    extractor: &RExtractor,
    left: Node,
) -> Option<(String, String)> {
    if left.kind() != "extract_operator"
        || extractor
            .base
            .get_node_text(&left.child_by_field_name("operator")?)
            != "$"
    {
        return None;
    }
    let receiver = extractor
        .base
        .get_node_text(&left.child_by_field_name("lhs")?);
    let member = assignment_name(extractor, left.child_by_field_name("rhs")?)?;
    Some((receiver, member))
}

pub(super) fn with_receiver(
    extractor: &mut RExtractor,
    symbol: Symbol,
    receiver: String,
) -> Symbol {
    let stored = extractor
        .symbols
        .iter_mut()
        .find(|stored| stored.id == symbol.id)
        .expect("function symbol was just pushed");
    stored
        .metadata
        .get_or_insert_with(HashMap::new)
        .insert("receiver".to_string(), serde_json::Value::String(receiver));
    stored.clone()
}

pub(super) fn inside_function(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "function_definition" {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// Names of same-file S3 generics: functions that call `UseMethod`, and `setGeneric` names.
pub(super) fn collect_same_file_generics(
    extractor: &RExtractor,
    root: Node,
) -> std::collections::HashSet<String> {
    let mut generics = std::collections::HashSet::new();
    collect_generics(extractor, root, 0, &mut generics);
    generics
}

fn collect_generics(
    extractor: &RExtractor,
    node: Node,
    depth: u32,
    generics: &mut std::collections::HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "binary_operator" => {
            if let (Some(lhs), Some(rhs)) = (
                node.child_by_field_name("lhs"),
                node.child_by_field_name("rhs"),
            ) && rhs.kind() == "function_definition"
                && rhs
                    .child_by_field_name("body")
                    .is_some_and(|body| extractor.base.get_node_text(&body).contains("UseMethod("))
                && let Some(name) = assignment_name(extractor, lhs)
            {
                generics.insert(name);
            }
        }
        "call" if call_name(extractor, node).as_deref() == Some("setGeneric") => {
            if let Some(name) = node
                .child_by_field_name("arguments")
                .and_then(|args| bind_arguments(extractor, args, &["name", "def"]).remove("name"))
                .and_then(|value| string_value(extractor, value))
            {
                generics.insert(name);
            }
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_generics(extractor, child, child_depth, generics);
    }
}

/// A class declaration's parent class, resolved to an `extends` edge after symbols exist.
pub(crate) struct ExtendsRequest {
    pub(crate) class_id: String,
    pub(crate) base: String,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
}

fn request_extends(extractor: &mut RExtractor, class_id: &str, base: String, site: Node) {
    extractor.extends_requests.push(ExtendsRequest {
        class_id: class_id.to_string(),
        base,
        start_byte: site.start_byte(),
        end_byte: site.end_byte(),
    });
}

/// Bind call arguments to formals the way R does: named arguments first,
/// then unnamed arguments fill the remaining formals in order.
pub(super) fn bind_arguments<'a>(
    extractor: &RExtractor,
    args: Node<'a>,
    formals: &[&'static str],
) -> HashMap<&'static str, Node<'a>> {
    let mut bound = HashMap::new();
    let mut unnamed = Vec::new();
    let mut cursor = args.walk();
    for argument in args.children_by_field_name("argument", &mut cursor) {
        let Some(value) = argument.child_by_field_name("value") else {
            continue;
        };
        match argument
            .child_by_field_name("name")
            .and_then(|name| clean_r_name(&extractor.base.get_node_text(&name)))
        {
            Some(name) => {
                if let Some(formal) = formals.iter().find(|formal| **formal == name) {
                    bound.insert(*formal, value);
                }
            }
            None => unnamed.push(value),
        }
    }
    let mut unnamed = unnamed.into_iter();
    for formal in formals {
        if !bound.contains_key(formal)
            && let Some(value) = unnamed.next()
        {
            bound.insert(*formal, value);
        }
    }
    bound
}

fn string_value(extractor: &RExtractor, value: Node) -> Option<String> {
    matches!(value.kind(), "string" | "identifier")
        .then(|| clean_r_name(&extractor.base.get_node_text(&value)))
        .flatten()
}

/// String values of `"A"`, `c("A", "B")`, `signature(x = "A")`, or `representation("A", ...)`.
/// With `unnamed_only`, named entries (slot declarations) are skipped.
fn string_values(extractor: &RExtractor, value: Node, unnamed_only: bool) -> Vec<String> {
    if let Some(single) = string_value(extractor, value).filter(|_| value.kind() == "string") {
        return vec![single];
    }
    if value.kind() != "call"
        || !matches!(
            call_name(extractor, value).as_deref(),
            Some("c" | "signature" | "representation" | "list")
        )
    {
        return Vec::new();
    }
    let Some(args) = value.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = args.walk();
    args.children_by_field_name("argument", &mut cursor)
        .filter(|argument| !unnamed_only || argument.child_by_field_name("name").is_none())
        .filter_map(|argument| argument.child_by_field_name("value"))
        .filter(|value| value.kind() == "string")
        .filter_map(|value| string_value(extractor, value))
        .collect()
}

pub(super) fn is_container_assignment(extractor: &RExtractor, left: Node, right: Node) -> bool {
    if right.kind() != "call" {
        return false;
    }

    let Some(name) = assignment_name(extractor, left) else {
        return false;
    };
    let Some(call_name) = call_name(extractor, right) else {
        return false;
    };

    matches!(name.as_str(), "public" | "private" | "fields" | "methods") && call_name == "list"
}

pub(super) fn extract_assignment_class_factory(
    extractor: &mut RExtractor,
    node: Node,
    assigned_name: &str,
    call: Node,
    parent_id: &Option<String>,
) -> Option<Symbol> {
    let call_name = call_name(extractor, call)?;
    let class_system = match call_name.as_str() {
        "R6Class" => "R6",
        "setRefClass" => "ReferenceClass",
        _ => return None,
    };

    let args = call.child(1);
    let class_name = args
        .and_then(|args| positional_string_argument(extractor, args, 0))
        .unwrap_or_else(|| assigned_name.to_string());

    let mut metadata = HashMap::new();
    metadata.insert(
        "r_class_system".to_string(),
        serde_json::Value::String(class_system.to_string()),
    );
    let parent_key = if class_system == "R6" {
        "inherit"
    } else {
        "contains"
    };
    let parent_value = call.child_by_field_name("arguments").and_then(|args| {
        let mut cursor = args.walk();
        args.children_by_field_name("argument", &mut cursor)
            .find(|argument| {
                argument
                    .child_by_field_name("name")
                    .and_then(|name| clean_r_name(&extractor.base.get_node_text(&name)))
                    .as_deref()
                    == Some(parent_key)
            })
            .and_then(|argument| argument.child_by_field_name("value"))
    });
    let parents: Vec<String> = parent_value
        .map(|value| {
            if class_system == "R6" {
                vec![extractor.base.get_node_text(&value)]
            } else {
                string_values(extractor, value, false)
            }
        })
        .unwrap_or_default();
    if !parents.is_empty() {
        metadata.insert(
            parent_key.to_string(),
            serde_json::Value::String(parents.join(",")),
        );
    }

    let symbol = extractor.base.create_symbol(
        &node,
        assigned_name.to_string(),
        SymbolKind::Class,
        SymbolOptions {
            parent_id: parent_id.clone(),
            signature: Some(format!("{assigned_name} <- {call_name}(\"{class_name}\")")),
            metadata: Some(metadata),
            doc_comment: extractor.base.find_doc_comment(&node),
            ..Default::default()
        },
    );
    extractor.symbols.push(symbol.clone());
    if let Some(site) = parent_value {
        for parent in parents {
            request_extends(extractor, &symbol.id, parent, site);
        }
    }
    extract_class_list_members(extractor, call, &symbol, class_system);
    Some(symbol)
}

pub(super) fn extract_s4_call(
    extractor: &mut RExtractor,
    node: Node,
    parent_id: &Option<String>,
) -> Option<Symbol> {
    let call_name = call_name(extractor, node)?;
    match call_name.as_str() {
        "setClass" => extract_s4_class(extractor, node, parent_id),
        "setGeneric" => extract_s4_generic(extractor, node, parent_id),
        "setMethod" => extract_s4_method(extractor, node, parent_id, ""),
        "setReplaceMethod" => extract_s4_method(extractor, node, parent_id, "<-"),
        _ => None,
    }
}

pub(super) fn extract_import_call(
    extractor: &mut RExtractor,
    node: Node,
    parent_id: &Option<String>,
) -> Option<Symbol> {
    let func_node = node.child(0)?;
    if func_node.kind() != "identifier" {
        return None;
    }
    let func_name = extractor.base.get_node_text(&func_node);
    if func_name != "library" && func_name != "require" && func_name != "source" {
        return None;
    }

    let args_node = node.child(1)?;
    let import_name = first_import_argument(extractor, args_node)?;
    if import_name.is_empty() {
        return None;
    }

    let signature = if func_name == "source" {
        extractor.base.get_node_text(&node)
    } else {
        format!("{}({})", func_name, import_name)
    };
    let symbol = extractor.base.create_symbol(
        &node,
        import_name,
        SymbolKind::Import,
        SymbolOptions {
            parent_id: parent_id.clone(),
            signature: Some(signature),
            doc_comment: extractor.base.find_doc_comment(&node),
            ..Default::default()
        },
    );
    extractor.symbols.push(symbol.clone());
    if func_name == "source" {
        emit_source_import_pending(extractor, &symbol, node);
    }
    Some(symbol)
}

pub(super) fn member_metadata(
    extractor: &RExtractor,
    node: Node,
    parent_id: &Option<String>,
) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();
    if let Some(parent_id) = parent_id
        && let Some(parent) = extractor
            .symbols
            .iter()
            .find(|symbol| symbol.id == *parent_id && symbol.kind == SymbolKind::Class)
        && let Some(class_system) = parent
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("r_class_system"))
            .and_then(|value| value.as_str())
    {
        metadata.insert(
            "r_class_system".to_string(),
            serde_json::Value::String(class_system.to_string()),
        );
    }
    if let Some(visibility) = enclosing_member_visibility(extractor, node) {
        metadata.insert(
            "member_visibility".to_string(),
            serde_json::Value::String(visibility),
        );
    }
    metadata
}

fn first_import_argument(extractor: &RExtractor, args_node: Node) -> Option<String> {
    let text = extractor.base.get_node_text(&args_node);
    split_top_level_arguments(&argument_list_text(&text))
        .into_iter()
        .find_map(|argument| {
            if argument.contains('=') {
                None
            } else {
                clean_r_name(&argument)
            }
        })
}

pub(super) fn emit_source_import_pending(extractor: &mut RExtractor, symbol: &Symbol, node: Node) {
    let target = UnresolvedTarget {
        display_name: symbol.name.clone(),
        terminal_name: symbol.name.clone(),
        receiver: None,
        namespace_path: Vec::new(),
        import_context: None,
    };
    let pending = extractor.base.create_pending_relationship(
        symbol.id.clone(),
        target,
        RelationshipKind::Imports,
        &node,
        Some(symbol.id.clone()),
        Some(1.0),
    );
    extractor.add_structured_pending_relationship(pending);
}

fn extract_s4_class(
    extractor: &mut RExtractor,
    node: Node,
    parent_id: &Option<String>,
) -> Option<Symbol> {
    let args = node.child_by_field_name("arguments")?;
    let bound = bind_arguments(
        extractor,
        args,
        &["Class", "representation", "prototype", "contains"],
    );
    let name = string_value(extractor, *bound.get("Class")?)?;
    let mut metadata = s4_metadata("class");
    if let Some(slots) = named_c_argument_names(extractor, args, "slots") {
        metadata.insert(
            "slots".to_string(),
            serde_json::Value::String(slots.join(",")),
        );
    }
    let contains_value = bound.get("contains").copied();
    let mut parents: Vec<(String, Node)> = contains_value
        .map(|value| {
            string_values(extractor, value, false)
                .into_iter()
                .map(|parent| (parent, value))
                .collect()
        })
        .unwrap_or_default();
    if let Some(representation) = bound.get("representation") {
        parents.extend(
            string_values(extractor, *representation, true)
                .into_iter()
                .filter(|parent| parent != "VIRTUAL")
                .map(|parent| (parent, *representation)),
        );
    }
    if !parents.is_empty() {
        let contains = parents
            .iter()
            .map(|(parent, _)| parent.as_str())
            .collect::<Vec<_>>()
            .join(",");
        metadata.insert("contains".to_string(), serde_json::Value::String(contains));
    }

    let symbol = extractor.base.create_symbol(
        &node,
        name.clone(),
        SymbolKind::Class,
        SymbolOptions {
            parent_id: parent_id.clone(),
            signature: Some(format!("setClass(\"{name}\")")),
            metadata: Some(metadata),
            doc_comment: extractor.base.find_doc_comment(&node),
            ..Default::default()
        },
    );
    extractor.symbols.push(symbol.clone());
    for (parent, site) in parents {
        request_extends(extractor, &symbol.id, parent, site);
    }
    Some(symbol)
}

fn extract_s4_generic(
    extractor: &mut RExtractor,
    node: Node,
    parent_id: &Option<String>,
) -> Option<Symbol> {
    let args = node.child_by_field_name("arguments")?;
    let name = string_value(
        extractor,
        *bind_arguments(extractor, args, &["name", "def"]).get("name")?,
    )?;
    let symbol = extractor.base.create_symbol(
        &node,
        name.clone(),
        SymbolKind::Function,
        SymbolOptions {
            parent_id: parent_id.clone(),
            signature: Some(format!("setGeneric(\"{name}\")")),
            metadata: Some(s4_metadata("generic")),
            doc_comment: extractor.base.find_doc_comment(&node),
            ..Default::default()
        },
    );
    extractor.symbols.push(symbol.clone());
    Some(symbol)
}

fn extract_s4_method(
    extractor: &mut RExtractor,
    node: Node,
    parent_id: &Option<String>,
    generic_suffix: &str,
) -> Option<Symbol> {
    let args = node.child_by_field_name("arguments")?;
    let bound = bind_arguments(extractor, args, &["f", "signature", "definition"]);
    let generic = format!(
        "{}{generic_suffix}",
        string_value(extractor, *bound.get("f")?)?
    );
    let classes = bound
        .get("signature")
        .map(|signature| string_values(extractor, *signature, false))
        .unwrap_or_default();
    let class_name = classes.join(",");
    let name = if class_name.is_empty() {
        generic.clone()
    } else {
        format!("{generic},{class_name}")
    };
    let mut metadata = s4_metadata("method");
    metadata.insert(
        "s4_generic".to_string(),
        serde_json::Value::String(generic.clone()),
    );
    if !class_name.is_empty() {
        metadata.insert(
            "s4_class".to_string(),
            serde_json::Value::String(class_name.clone()),
        );
    }

    let options = SymbolOptions {
        parent_id: parent_id.clone(),
        signature: Some(format!("setMethod(\"{generic}\", \"{class_name}\")")),
        metadata: Some(metadata),
        doc_comment: extractor.base.find_doc_comment(&node),
        ..Default::default()
    };
    let definition = bound
        .get("definition")
        .copied()
        .filter(|definition| definition.kind() == "function_definition");
    let symbol = match definition {
        Some(definition) => extractor.base.create_symbol_from_span(
            &definition,
            crate::base::NormalizedSpan::from_node(&node),
            name,
            SymbolKind::Method,
            options,
        ),
        None => extractor
            .base
            .create_symbol(&node, name, SymbolKind::Method, options),
    };
    extractor.symbols.push(symbol.clone());
    if let Some(definition) = definition {
        extractor
            .value_owners
            .insert(definition.id(), symbol.id.clone());
    }
    Some(symbol)
}

fn s4_metadata(role: &str) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();
    metadata.insert(
        "r_class_system".to_string(),
        serde_json::Value::String("S4".to_string()),
    );
    metadata.insert(
        "s4_role".to_string(),
        serde_json::Value::String(role.to_string()),
    );
    metadata
}

fn enclosing_member_visibility(extractor: &RExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    for _ in 0..8 {
        let parent = current?;
        if parent.kind() == "binary_operator"
            && let Some(left) = parent.child(0)
        {
            let text = assignment_name(extractor, left)?;
            if text == "public" || text == "private" {
                return Some(text);
            }
        }
        current = parent.parent();
    }
    None
}

pub(super) fn call_name(extractor: &RExtractor, call: Node) -> Option<String> {
    let callee = call.child_by_field_name("function")?;
    match callee.kind() {
        "identifier" => clean_r_name(&extractor.base.get_node_text(&callee)),
        "namespace_operator" => {
            let rhs = callee.child_by_field_name("rhs")?;
            clean_r_name(&extractor.base.get_node_text(&rhs))
        }
        _ => None,
    }
}

pub(super) fn positional_string_argument(
    extractor: &RExtractor,
    args: Node,
    index: usize,
) -> Option<String> {
    let mut cursor = args.walk();
    let value = args
        .children_by_field_name("argument", &mut cursor)
        .filter(|argument| argument.child_by_field_name("name").is_none())
        .nth(index)?
        .child_by_field_name("value")?;
    string_value(extractor, value)
}

fn named_argument_value(extractor: &RExtractor, args: Node, name: &str) -> Option<String> {
    find_named_argument_value(extractor, args, name)
}

fn find_named_argument_value(extractor: &RExtractor, node: Node, name: &str) -> Option<String> {
    find_named_argument_value_at_depth(extractor, node, name, 0)
}

fn find_named_argument_value_at_depth(
    extractor: &RExtractor,
    node: Node,
    name: &str,
    depth: u32,
) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    if node.kind() == "argument" {
        let argument_name = node.child_by_field_name("name")?;
        let value = node.child_by_field_name("value")?;
        if assignment_name(extractor, argument_name).as_deref() == Some(name) {
            return Some(extractor.base.get_node_text(&value));
        }
    }

    if node.kind() == "binary_operator" {
        let left = node.child(0)?;
        let op = node.child(1)?;
        let right = node.child(2)?;
        if extractor.base.get_node_text(&op) == "="
            && assignment_name(extractor, left).as_deref() == Some(name)
        {
            return Some(extractor.base.get_node_text(&right));
        }
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(value) = find_named_argument_value_at_depth(extractor, child, name, child_depth)
        {
            return Some(value);
        }
    }
    None
}

fn named_c_argument_names(extractor: &RExtractor, args: Node, name: &str) -> Option<Vec<String>> {
    let text = named_argument_value(extractor, args, name)?;
    let start = text.find('(')? + 1;
    let end = text.rfind(')')?;
    let inner = &text[start..end];
    let names = inner
        .split(',')
        .filter_map(|entry| {
            entry
                .split_once('=')
                .map(|(slot, _)| slot.trim().to_string())
        })
        .filter(|slot| !slot.is_empty())
        .collect::<Vec<_>>();
    if names.is_empty() { None } else { Some(names) }
}

fn extract_class_list_members(
    extractor: &mut RExtractor,
    call: Node,
    class_symbol: &Symbol,
    class_system: &str,
) {
    let Some(args) = call.child_by_field_name("arguments") else {
        return;
    };
    let mut cursor = args.walk();
    let member_lists = args
        .children_by_field_name("argument", &mut cursor)
        .filter_map(|argument| {
            let visibility = member_list_visibility(extractor, argument)?;
            let list_call = argument.child_by_field_name("value")?;
            if list_call.kind() != "call"
                || call_name(extractor, list_call).as_deref() != Some("list")
            {
                return None;
            }
            Some((list_call, visibility))
        })
        .collect::<Vec<_>>();

    for (list_call, member_visibility) in member_lists {
        let Some(list_args) = list_call.child_by_field_name("arguments") else {
            continue;
        };
        let mut cursor = list_args.walk();
        let members = list_args
            .children_by_field_name("argument", &mut cursor)
            .collect::<Vec<_>>();
        for member in members {
            extract_class_list_member(
                extractor,
                member,
                class_symbol,
                class_system,
                member_visibility,
            );
        }
    }
}

fn member_list_visibility(extractor: &RExtractor, argument: Node) -> Option<&'static str> {
    let name_node = argument.child_by_field_name("name")?;
    match clean_r_name(&extractor.base.get_node_text(&name_node))?.as_str() {
        "private" => Some("private"),
        "public" | "fields" | "methods" => Some("public"),
        _ => None,
    }
}

fn extract_class_list_member(
    extractor: &mut RExtractor,
    member: Node,
    class_symbol: &Symbol,
    class_system: &str,
    member_visibility: &str,
) {
    let Some(name_node) = member.child_by_field_name("name") else {
        return;
    };
    let Some(name) = clean_r_name(&extractor.base.get_node_text(&name_node)) else {
        return;
    };
    let Some(value) = member.child_by_field_name("value") else {
        return;
    };
    let value_text = extractor.base.get_node_text(&value);
    let is_method = value.kind() == "function_definition";
    let kind = if is_method {
        SymbolKind::Method
    } else {
        SymbolKind::Field
    };
    let mut metadata = HashMap::new();
    metadata.insert(
        "r_class_system".to_string(),
        serde_json::Value::String(class_system.to_string()),
    );
    metadata.insert(
        "member_visibility".to_string(),
        serde_json::Value::String(member_visibility.to_string()),
    );
    let signature = if is_method {
        format!("{name} = {}", function_signature(&value_text))
    } else {
        format!("{name} = {}", value_text.trim())
    };
    let options = SymbolOptions {
        parent_id: Some(class_symbol.id.clone()),
        signature: Some(signature),
        metadata: Some(metadata),
        ..Default::default()
    };
    let symbol = if is_method {
        extractor.base.create_symbol_from_span(
            &value,
            crate::base::NormalizedSpan::from_node(&member),
            name,
            kind,
            options,
        )
    } else {
        extractor.base.create_symbol(&member, name, kind, options)
    };
    if is_method {
        extractor.value_owners.insert(value.id(), symbol.id.clone());
    }
    extractor.symbols.push(symbol);
}

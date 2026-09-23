use crate::base::{RelationshipKind, Symbol, SymbolKind, SymbolOptions, UnresolvedTarget};
use crate::r::RExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::text_args::{clean_r_name, function_signature};

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
        "new_class" => "S7",
        "new_generic" => {
            return Some(extract_s7_generic(
                extractor,
                node,
                assigned_name,
                parent_id,
            ));
        }
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
    let parent_key = match class_system {
        "R6" => "inherit",
        "S7" => "parent",
        _ => "contains",
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
            if class_system != "ReferenceClass" {
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
            visibility: Some(name_visibility(assigned_name)),
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
    if class_system == "S7"
        && let Some(properties) = call
            .child_by_field_name("arguments")
            .and_then(|args| named_argument_node(extractor, args, "properties"))
    {
        extract_typed_fields(extractor, properties, &symbol, class_system);
    }
    Some(symbol)
}

/// `speak <- new_generic("speak", "x")`: an S7 generic function.
fn extract_s7_generic(
    extractor: &mut RExtractor,
    node: Node,
    assigned_name: &str,
    parent_id: &Option<String>,
) -> Symbol {
    let mut metadata = HashMap::new();
    metadata.insert(
        "r_class_system".to_string(),
        serde_json::Value::String("S7".to_string()),
    );
    metadata.insert(
        "s7_role".to_string(),
        serde_json::Value::String("generic".to_string()),
    );
    let symbol = extractor.base.create_symbol(
        &node,
        assigned_name.to_string(),
        SymbolKind::Function,
        SymbolOptions {
            parent_id: parent_id.clone(),
            signature: Some(format!(
                "{assigned_name} <- new_generic(\"{assigned_name}\")"
            )),
            metadata: Some(metadata),
            doc_comment: extractor.base.find_doc_comment(&node),
            visibility: Some(name_visibility(assigned_name)),
            ..Default::default()
        },
    );
    extractor.symbols.push(symbol.clone());
    symbol
}

/// `method(generic, Class) <- function(...)`: an S7 method named `generic,Class`.
pub(super) fn extract_s7_method(
    extractor: &mut RExtractor,
    node: Node,
    target: Node,
    definition: Node,
    parent_id: &Option<String>,
) -> Option<Symbol> {
    if target.kind() != "call" || call_name(extractor, target).as_deref() != Some("method") {
        return None;
    }
    let args = target.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let parts: Vec<String> = args
        .children_by_field_name("argument", &mut cursor)
        .filter_map(|argument| argument.child_by_field_name("value"))
        .map(|value| {
            let text = extractor.base.get_node_text(&value);
            text.rsplit("::").next().unwrap_or(&text).to_string()
        })
        .collect();
    let (generic, classes) = parts.split_first()?;
    let class_name = classes.join(",");
    let mut metadata = HashMap::new();
    metadata.insert(
        "r_class_system".to_string(),
        serde_json::Value::String("S7".to_string()),
    );
    metadata.insert(
        "s7_generic".to_string(),
        serde_json::Value::String(generic.clone()),
    );
    metadata.insert(
        "s7_class".to_string(),
        serde_json::Value::String(class_name.clone()),
    );
    let symbol = extractor.base.create_symbol_from_span(
        &definition,
        crate::base::NormalizedSpan::from_node(&node),
        format!("{generic},{class_name}"),
        SymbolKind::Method,
        SymbolOptions {
            parent_id: parent_id.clone(),
            signature: Some(format!(
                "method({generic}, {class_name}) <- {}",
                function_signature(&extractor.base.get_node_text(&definition))
            )),
            metadata: Some(metadata),
            doc_comment: extractor.base.find_doc_comment(&node),
            visibility: Some(crate::base::Visibility::Public),
            ..Default::default()
        },
    );
    extractor.symbols.push(symbol.clone());
    extractor
        .value_owners
        .insert(definition.id(), symbol.id.clone());
    Some(symbol)
}

/// Public unless the name starts with a dot, R's convention for internal objects.
pub(super) fn name_visibility(name: &str) -> crate::base::Visibility {
    if name.starts_with('.') {
        crate::base::Visibility::Private
    } else {
        crate::base::Visibility::Public
    }
}

/// The doc comment of a declaring call, or of the assignment that binds it.
fn declaration_doc(extractor: &RExtractor, node: Node) -> Option<String> {
    extractor.base.find_doc_comment(&node).or_else(|| {
        node.parent()
            .filter(|parent| {
                parent.kind() == "binary_operator"
                    && parent.child_by_field_name("rhs") == Some(node)
            })
            .and_then(|parent| extractor.base.find_doc_comment(&parent))
    })
}

fn named_argument_node<'a>(extractor: &RExtractor, args: Node<'a>, name: &str) -> Option<Node<'a>> {
    let mut cursor = args.walk();
    args.children_by_field_name("argument", &mut cursor)
        .find(|argument| {
            argument
                .child_by_field_name("name")
                .and_then(|n| clean_r_name(&extractor.base.get_node_text(&n)))
                .as_deref()
                == Some(name)
        })
        .and_then(|argument| argument.child_by_field_name("value"))
}

/// Field symbols for the entries of `c(...)`, `list(...)`, or `representation(...)`:
/// `name = "type"` declares a typed field, `name = S7::class_x` an S7 property,
/// and an unnamed `"name"` an untyped field.
fn extract_typed_fields(
    extractor: &mut RExtractor,
    declarations: Node,
    class_symbol: &Symbol,
    class_system: &str,
) {
    if declarations.kind() != "call"
        || !matches!(
            call_name(extractor, declarations).as_deref(),
            Some("c" | "list" | "representation")
        )
    {
        return;
    }
    let Some(args) = declarations.child_by_field_name("arguments") else {
        return;
    };
    let mut cursor = args.walk();
    let entries: Vec<Node> = args
        .children_by_field_name("argument", &mut cursor)
        .collect();
    for entry in entries {
        let Some(value) = entry.child_by_field_name("value") else {
            continue;
        };
        let named = entry
            .child_by_field_name("name")
            .and_then(|name| clean_r_name(&extractor.base.get_node_text(&name)));
        let (name, type_name) = match named {
            Some(name) => (name, declared_field_type(extractor, value)),
            None if value.kind() == "string" => match string_value(extractor, value) {
                Some(name) => (name, None),
                None => continue,
            },
            None => continue,
        };
        push_field(
            extractor,
            entry,
            name,
            type_name,
            class_symbol,
            class_system,
        );
    }
}

fn declared_field_type(extractor: &RExtractor, value: Node) -> Option<String> {
    if value.kind() == "string" {
        return string_value(extractor, value);
    }
    let text = extractor.base.get_node_text(&value);
    let bare = text.rsplit("::").next().unwrap_or(&text).trim();
    let bare = bare.strip_prefix("class_").unwrap_or(bare);
    (!bare.is_empty()
        && bare
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.'))
    .then(|| bare.to_string())
}

fn push_field(
    extractor: &mut RExtractor,
    entry: Node,
    name: String,
    type_name: Option<String>,
    class_symbol: &Symbol,
    class_system: &str,
) {
    let mut metadata = HashMap::new();
    metadata.insert(
        "r_class_system".to_string(),
        serde_json::Value::String(class_system.to_string()),
    );
    let symbol = extractor.base.create_symbol(
        &entry,
        name.clone(),
        SymbolKind::Field,
        SymbolOptions {
            parent_id: Some(class_symbol.id.clone()),
            signature: Some(extractor.base.get_node_text(&entry)),
            metadata: Some(metadata),
            visibility: Some(name_visibility(&name)),
            ..Default::default()
        },
    );
    if let Some(type_name) = type_name {
        extractor.base.record_declared_type_fact(
            &symbol.id,
            &type_name,
            &super::type_facts::R_TYPE_NAME_RULES,
            false,
        );
    }
    extractor.symbols.push(symbol);
}

/// S4 slots: only named entries declare slots; unnamed `representation()`
/// entries name superclasses.
fn extract_typed_slots(extractor: &mut RExtractor, slots: Node, class_symbol: &Symbol) {
    if slots.kind() != "call"
        || !matches!(
            call_name(extractor, slots).as_deref(),
            Some("c" | "list" | "representation")
        )
    {
        return;
    }
    let Some(args) = slots.child_by_field_name("arguments") else {
        return;
    };
    let mut cursor = args.walk();
    let entries: Vec<Node> = args
        .children_by_field_name("argument", &mut cursor)
        .collect();
    for entry in entries {
        let (Some(name), Some(value)) = (
            entry
                .child_by_field_name("name")
                .and_then(|name| clean_r_name(&extractor.base.get_node_text(&name))),
            entry.child_by_field_name("value"),
        ) else {
            continue;
        };
        let type_name = declared_field_type(extractor, value);
        push_field(extractor, entry, name, type_name, class_symbol, "S4");
    }
}

/// `Class$methods(name = function(...), ...)`: RefClass methods added after the
/// generator, parented to the same-file class.
pub(super) fn extract_refclass_methods(extractor: &mut RExtractor, call: Node) -> bool {
    let Some(callee) = call.child_by_field_name("function") else {
        return false;
    };
    if callee.kind() != "extract_operator"
        || callee
            .child_by_field_name("rhs")
            .is_none_or(|rhs| extractor.base.get_node_text(&rhs) != "methods")
    {
        return false;
    }
    let Some(class_name) = callee
        .child_by_field_name("lhs")
        .map(|lhs| extractor.base.get_node_text(&lhs))
    else {
        return false;
    };
    let Some(class_symbol) = extractor
        .symbols
        .iter()
        .rev()
        .find(|symbol| {
            symbol.kind == SymbolKind::Class
                && symbol.name == class_name
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("r_class_system"))
                    .and_then(|value| value.as_str())
                    == Some("ReferenceClass")
        })
        .cloned()
    else {
        return false;
    };
    let Some(args) = call.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = args.walk();
    let mut members: Vec<Node> = args
        .children_by_field_name("argument", &mut cursor)
        .collect();
    if let [single] = members.as_slice()
        && single.child_by_field_name("name").is_none()
        && let Some(list_args) = single
            .child_by_field_name("value")
            .filter(|value| call_name(extractor, *value).as_deref() == Some("list"))
            .and_then(|value| value.child_by_field_name("arguments"))
    {
        let mut cursor = list_args.walk();
        members = list_args
            .children_by_field_name("argument", &mut cursor)
            .collect();
    }
    for member in members {
        extract_class_list_member(
            extractor,
            member,
            &class_symbol,
            "ReferenceClass",
            MemberList::Public,
        );
    }
    true
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
    let (form, modules) = import_modules(extractor, node)?;
    let mut first = None;
    for module in modules {
        let signature = if form == "source" {
            extractor.base.get_node_text(&node)
        } else {
            format!("{form}({module})")
        };
        let symbol = extractor.base.create_symbol(
            &node,
            module,
            SymbolKind::Import,
            SymbolOptions {
                parent_id: parent_id.clone(),
                signature: Some(signature),
                doc_comment: extractor.base.find_doc_comment(&node),
                ..Default::default()
            },
        );
        extractor.symbols.push(symbol.clone());
        if form == "source" {
            emit_source_import_pending(extractor, &symbol, node);
        }
        first.get_or_insert(symbol);
    }
    first
}

/// The load form and module names of a package or file load call:
/// `library`/`require`/`requireNamespace(pkg)`, `source(path)` (a string or a
/// `file.path()` of strings), `pacman::p_load(a, b)`, `box::use(pkg[...], a/b)`,
/// and `import::from(pkg, ...)`.
pub(super) fn import_modules(extractor: &RExtractor, call: Node) -> Option<(String, Vec<String>)> {
    let callee = call.child_by_field_name("function")?;
    let args = call.child_by_field_name("arguments")?;
    let form = match callee.kind() {
        "identifier" => extractor.base.get_node_text(&callee),
        "namespace_operator" => {
            let lhs = extractor
                .base
                .get_node_text(&callee.child_by_field_name("lhs")?);
            let rhs = extractor
                .base
                .get_node_text(&callee.child_by_field_name("rhs")?);
            format!("{lhs}::{rhs}")
        }
        _ => return None,
    };
    let mut cursor = args.walk();
    let unnamed: Vec<Node> = args
        .children_by_field_name("argument", &mut cursor)
        .filter(|argument| argument.child_by_field_name("name").is_none())
        .filter_map(|argument| argument.child_by_field_name("value"))
        .collect();
    let package = |value: &Node| {
        matches!(value.kind(), "identifier" | "string")
            .then(|| clean_r_name(&extractor.base.get_node_text(value)))
            .flatten()
    };
    let modules: Vec<String> = match form.as_str() {
        "library" | "require" | "import::from" => {
            unnamed.first().and_then(package).into_iter().collect()
        }
        "requireNamespace" => unnamed
            .first()
            .filter(|value| value.kind() == "string")
            .and_then(package)
            .into_iter()
            .collect(),
        "pacman::p_load" => unnamed.iter().filter_map(package).collect(),
        "box::use" => unnamed
            .iter()
            .filter_map(|value| box_module(extractor, *value))
            .collect(),
        "source" => unnamed
            .first()
            .and_then(|value| source_path(extractor, *value))
            .into_iter()
            .collect(),
        "import" if is_namespace_file(&extractor.base.file_path) => {
            unnamed.iter().filter_map(package).collect()
        }
        "importFrom" | "importClassesFrom" | "importMethodsFrom"
            if is_namespace_file(&extractor.base.file_path) =>
        {
            unnamed.first().and_then(package).into_iter().collect()
        }
        _ => return None,
    };
    (!modules.is_empty()).then_some((form, modules))
}

/// An R package `NAMESPACE` file, which holds `import()` / `export()` directives.
pub(crate) fn is_namespace_file(file_path: &str) -> bool {
    file_path.rsplit(['/', '\\']).next() == Some("NAMESPACE")
}

/// `pkg`, `pkg[a, b]`, or `app/logic/utils` in a `box::use()` call.
fn box_module(extractor: &RExtractor, value: Node) -> Option<String> {
    match value.kind() {
        "identifier" => clean_r_name(&extractor.base.get_node_text(&value)),
        "subset" => box_module(extractor, value.child_by_field_name("function")?),
        "binary_operator"
            if value
                .child_by_field_name("operator")
                .is_some_and(|op| extractor.base.get_node_text(&op) == "/") =>
        {
            let lhs = box_module(extractor, value.child_by_field_name("lhs")?)?;
            let rhs = box_module(extractor, value.child_by_field_name("rhs")?)?;
            Some(format!("{lhs}/{rhs}"))
        }
        _ => None,
    }
}

/// A static `source()` path: a string, or `file.path()` of strings joined with `/`.
fn source_path(extractor: &RExtractor, value: Node) -> Option<String> {
    if value.kind() == "string" {
        return string_value(extractor, value);
    }
    if value.kind() != "call" || call_name(extractor, value).as_deref() != Some("file.path") {
        return None;
    }
    let args = value.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let parts: Option<Vec<String>> = args
        .children_by_field_name("argument", &mut cursor)
        .filter(|argument| argument.child_by_field_name("name").is_none())
        .map(|argument| {
            argument
                .child_by_field_name("value")
                .filter(|part| part.kind() == "string")
                .and_then(|part| string_value(extractor, part))
        })
        .collect();
    parts
        .filter(|parts| !parts.is_empty())
        .map(|parts| parts.join("/"))
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
            doc_comment: declaration_doc(extractor, node),
            visibility: Some(name_visibility(&name)),
            ..Default::default()
        },
    );
    extractor.symbols.push(symbol.clone());
    for (parent, site) in parents {
        request_extends(extractor, &symbol.id, parent, site);
    }
    for slots in [
        named_argument_node(extractor, args, "slots"),
        bound.get("representation").copied(),
    ]
    .into_iter()
    .flatten()
    {
        extract_typed_slots(extractor, slots, &symbol);
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
            doc_comment: declaration_doc(extractor, node),
            visibility: Some(name_visibility(&name)),
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
        doc_comment: declaration_doc(extractor, node),
        visibility: Some(crate::base::Visibility::Public),
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
            let list_kind = call_name(extractor, list_call);
            let is_list = list_call.kind() == "call" && list_kind.as_deref() == Some("list");
            let is_vector = list_call.kind() == "call" && list_kind.as_deref() == Some("c");
            if !is_list
                && !(visibility == MemberList::Fields
                    && (is_vector || list_call.kind() == "string"))
            {
                return None;
            }
            Some((list_call, visibility))
        })
        .collect::<Vec<_>>();

    for (list_call, member_visibility) in member_lists {
        if member_visibility == MemberList::Fields && class_system == "ReferenceClass" {
            if list_call.kind() == "string" {
                if let Some(name) = string_value(extractor, list_call) {
                    push_field(extractor, list_call, name, None, class_symbol, class_system);
                }
            } else {
                extract_typed_fields(extractor, list_call, class_symbol, class_system);
            }
            continue;
        }
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

/// Which member list of a class generator an argument declares.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MemberList {
    Public,
    Private,
    Active,
    Fields,
}

impl MemberList {
    fn visibility(self) -> &'static str {
        if self == MemberList::Private {
            "private"
        } else {
            "public"
        }
    }
}

fn member_list_visibility(extractor: &RExtractor, argument: Node) -> Option<MemberList> {
    let name_node = argument.child_by_field_name("name")?;
    match clean_r_name(&extractor.base.get_node_text(&name_node))?.as_str() {
        "private" => Some(MemberList::Private),
        "public" | "methods" => Some(MemberList::Public),
        "active" => Some(MemberList::Active),
        "fields" => Some(MemberList::Fields),
        _ => None,
    }
}

fn extract_class_list_member(
    extractor: &mut RExtractor,
    member: Node,
    class_symbol: &Symbol,
    class_system: &str,
    member_list: MemberList,
) {
    let member_visibility = member_list.visibility();
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
    let kind = if member_list == MemberList::Active {
        SymbolKind::Property
    } else if is_method {
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
    if member_list == MemberList::Active {
        metadata.insert(
            "r6_member_kind".to_string(),
            serde_json::Value::String("active".to_string()),
        );
    }
    let visibility = if member_list == MemberList::Private {
        crate::base::Visibility::Private
    } else {
        crate::base::Visibility::Public
    };
    let signature = if is_method {
        format!("{name} = {}", function_signature(&value_text))
    } else {
        format!("{name} = {}", value_text.trim())
    };
    let options = SymbolOptions {
        parent_id: Some(class_symbol.id.clone()),
        signature: Some(signature),
        metadata: Some(metadata),
        visibility: Some(visibility),
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

/// An anonymous top-level function under a plumber `#* @get /path` block: a
/// route handler named after its first route (`GET /path`).
pub(super) fn extract_plumber_handler(
    extractor: &mut RExtractor,
    node: Node,
    parent_id: &Option<String>,
) -> Option<Symbol> {
    if node.parent()?.kind() != "program" {
        return None;
    }
    let routes = super::plumber::annotated_routes(&extractor.base.content, node);
    let (verb, path) = routes.first()?;
    let name = format!("{verb} {path}");
    let signature = format!(
        "{name} <- {}",
        function_signature(&extractor.base.get_node_text(&node))
    );
    let symbol = extractor.base.create_symbol(
        &node,
        name,
        SymbolKind::Function,
        SymbolOptions {
            parent_id: parent_id.clone(),
            signature: Some(signature),
            visibility: Some(crate::base::Visibility::Public),
            ..Default::default()
        },
    );
    extractor.symbols.push(symbol.clone());
    let parameters = super::parameters::extract_parameter_symbols(extractor, node, &symbol.id);
    extractor.symbols.extend(parameters);
    Some(symbol)
}

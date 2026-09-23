//! Swift framework facts: Vapor routes (`vapor.route.v1`) and the SwiftPM
//! `Package.swift` manifest (`swiftpm.*.v1`, `manifest.dependency.v1`).
//!
//! Vapor gate: the file imports Vapor, the call is `<builder>.<verb>(...)` or
//! `<builder>.on(.VERB, ...)`, every positional argument is a static string
//! path component, and the call passes a handler (`use:` or a trailing
//! closure). The builder's prefix comes from same-file `grouped(...)` bindings
//! and `group(...) { builder in }` closures; anything else is the root.
use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::helpers::{fact_for_node, insert_string, insert_string_array};
use super::scan::{RouteFactSpec, route_fact};
use super::{MANIFEST_DEPENDENCY_PATTERN_ID, VAPOR_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const SWIFTPM_PACKAGE_PATTERN_ID: &str = "swiftpm.package.v1";
const SWIFTPM_PRODUCT_PATTERN_ID: &str = "swiftpm.product.v1";
const SWIFTPM_TARGET_PATTERN_ID: &str = "swiftpm.target.v1";

/// Bound on `grouped` binding hops, so a self-referencing binding terminates.
const MAX_PREFIX_HOPS: u32 = 16;

pub(super) fn collect_swift_framework_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let root = tree.root_node();
    if imports_module(root, content, "Vapor") {
        walk_vapor(root, language, tree, file_path, content, 0, &mut facts);
    }
    if is_package_manifest(file_path) {
        walk_manifest(root, language, file_path, content, 0, &mut facts);
    }
    facts
}

fn imports_module(root: Node, content: &str, module: &str) -> bool {
    let mut cursor = root.walk();
    root.named_children(&mut cursor).any(|child| {
        child.kind() == "import_declaration"
            && child
                .named_children(&mut child.walk())
                .find(|part| part.kind() == "identifier")
                .and_then(|path| content.get(path.start_byte()..path.end_byte()))
                == Some(module)
    })
}

fn is_package_manifest(file_path: &str) -> bool {
    file_path.rsplit(['/', '\\']).next() == Some("Package.swift")
}

/// The text of a string literal with no interpolation.
pub(super) fn swift_static_string<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    if node.kind() != "line_string_literal" {
        return None;
    }
    let mut cursor = node.walk();
    if node
        .named_children(&mut cursor)
        .any(|child| !matches!(child.kind(), "line_str_text" | "str_escaped_char"))
    {
        return None;
    }
    let text = content.get(node.start_byte()..node.end_byte())?;
    text.strip_prefix('"')?.strip_suffix('"')
}

/// The `call_suffix` arguments of a call: `(label, value)` pairs in order.
pub(super) fn swift_call_arguments<'t>(
    call: Node<'t>,
    content: &str,
) -> Vec<(Option<String>, Node<'t>)> {
    let Some(suffix) = named_child_of_kind(call, "call_suffix") else {
        return Vec::new();
    };
    let Some(arguments) = named_child_of_kind(suffix, "value_arguments") else {
        return Vec::new();
    };
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .filter(|argument| argument.kind() == "value_argument")
        .filter_map(|argument| {
            let value = argument.child_by_field_name("value")?;
            let label = argument
                .child_by_field_name("name")
                .and_then(|name| content.get(name.start_byte()..name.end_byte()))
                .map(str::to_string);
            Some((label, value))
        })
        .collect()
}

/// `receiver.method` of a call whose callee is a navigation expression.
pub(super) fn swift_method_call<'t>(
    call: Node<'t>,
    content: &'t str,
) -> Option<(Node<'t>, &'t str)> {
    if call.kind() != "call_expression" {
        return None;
    }
    let callee = call.named_child(0)?;
    if callee.kind() != "navigation_expression" {
        return None;
    }
    let receiver = callee.child_by_field_name("target")?;
    let name = callee
        .child_by_field_name("suffix")?
        .child_by_field_name("suffix")?;
    Some((receiver, content.get(name.start_byte()..name.end_byte())?))
}

pub(super) fn named_child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn walk_vapor(
    node: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if let Some(fact) = vapor_route_fact(node, language, tree, file_path, content) {
        facts.push(fact);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_vapor(
            child,
            language,
            tree,
            file_path,
            content,
            child_depth,
            facts,
        );
    }
}

fn vapor_route_fact(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let (receiver, method) = swift_method_call(call, content)?;
    let arguments = swift_call_arguments(call, content);
    let mut positional = arguments.iter().filter(|(label, _)| label.is_none());
    let verb = match method {
        "get" => "GET",
        "post" => "POST",
        "put" => "PUT",
        "patch" => "PATCH",
        "delete" => "DELETE",
        "on" => http_method_member(positional.next()?.1, content)?,
        _ => return None,
    };
    let components = positional
        .map(|(_, value)| swift_static_string(*value, content))
        .collect::<Option<Vec<_>>>()?;
    let handler = arguments
        .iter()
        .find(|(label, _)| label.as_deref() == Some("use"))
        .and_then(|(_, value)| content.get(value.start_byte()..value.end_byte()));
    let has_closure = named_child_of_kind(call, "call_suffix")
        .and_then(|suffix| named_child_of_kind(suffix, "lambda_literal"))
        .is_some();
    if handler.is_none() && !has_closure {
        return None;
    }
    let route_template = format!("/{}", components.join("/"));
    let prefix = builder_prefix(receiver, content, MAX_PREFIX_HOPS)?;
    let effective = (!prefix.is_empty()).then(|| {
        if components.is_empty() {
            format!("/{}", prefix.join("/"))
        } else {
            format!("/{}/{}", prefix.join("/"), components.join("/"))
        }
    });
    let spec = RouteFactSpec {
        framework: "vapor",
        pattern_id: VAPOR_ROUTE_PATTERN_ID,
        capture_name: "route",
        api_style: "route_builder",
        route_template: &route_template,
        verb: Some(verb),
        verb_source: Some("attested"),
        flavor: ParamFlavor::Colon,
        prefix: None,
        prefix_key: None,
    };
    route_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        spec,
        |metadata| {
            if let Some(effective) = &effective {
                insert_string(metadata, "effective_route_template", effective);
                let normalized = normalize_route_template(effective, ParamFlavor::Colon);
                insert_string(metadata, "normalized_route_template", &normalized.template);
                metadata.remove("dynamic_segments");
                if !normalized.dynamic_segments.is_empty() {
                    insert_string_array(metadata, "dynamic_segments", normalized.dynamic_segments);
                }
            }
            if let Some(handler) = handler {
                insert_string(metadata, "handler", handler);
            }
        },
    )
}

/// `.GET` in `app.on(.GET, "path")`.
fn http_method_member(node: Node, content: &str) -> Option<&'static str> {
    let text = content.get(node.start_byte()..node.end_byte())?;
    match text.strip_prefix('.')? {
        "GET" => Some("GET"),
        "POST" => Some("POST"),
        "PUT" => Some("PUT"),
        "PATCH" => Some("PATCH"),
        "DELETE" => Some("DELETE"),
        "HEAD" => Some("HEAD"),
        "OPTIONS" => Some("OPTIONS"),
        _ => None,
    }
}

/// The static path components a route builder expression prefixes. `None`
/// when a `grouped`/`group` segment is dynamic, so no fact is published.
fn builder_prefix(builder: Node, content: &str, hops: u32) -> Option<Vec<String>> {
    if hops == 0 {
        return Some(Vec::new());
    }
    if let Some((receiver, "grouped")) = swift_method_call(builder, content) {
        let mut prefix = builder_prefix(receiver, content, hops - 1)?;
        prefix.extend(group_components(builder, content)?);
        return Some(prefix);
    }
    if builder.kind() != "simple_identifier" {
        return Some(Vec::new());
    }
    let name = content.get(builder.start_byte()..builder.end_byte())?;
    match builder_binding(builder, name, content) {
        Some(Binding::Value(value)) => builder_prefix(value, content, hops - 1),
        Some(Binding::GroupClosure(group_call, receiver)) => {
            let mut prefix = builder_prefix(receiver, content, hops - 1)?;
            prefix.extend(group_components(group_call, content)?);
            Some(prefix)
        }
        None => Some(Vec::new()),
    }
}

/// The string path components of a `grouped(...)`/`group(...)` call; other
/// arguments (middleware) do not add path.
fn group_components(call: Node, content: &str) -> Option<Vec<String>> {
    swift_call_arguments(call, content)
        .into_iter()
        .filter(|(label, _)| label.is_none())
        .filter(|(_, value)| value.kind() == "line_string_literal")
        .map(|(_, value)| swift_static_string(value, content).map(str::to_string))
        .collect()
}

enum Binding<'t> {
    /// `let name = <value>` in an enclosing statement list, before the use.
    Value(Node<'t>),
    /// `receiver.group(...) { name in ... }`.
    GroupClosure(Node<'t>, Node<'t>),
}

fn builder_binding<'t>(usage: Node<'t>, name: &str, content: &'t str) -> Option<Binding<'t>> {
    let mut current = usage;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "statements" => {
                let mut cursor = parent.walk();
                let binding = parent
                    .named_children(&mut cursor)
                    .take_while(|statement| statement.start_byte() < usage.start_byte())
                    .filter(|statement| statement.kind() == "property_declaration")
                    .filter(|statement| binds_name(*statement, name, content))
                    .last()
                    .and_then(|statement| statement.child_by_field_name("value"));
                if let Some(value) = binding {
                    return Some(Binding::Value(value));
                }
            }
            "lambda_literal" if lambda_binds(parent, name, content) => {
                let group_call = parent.parent()?.parent()?;
                let (receiver, method) = swift_method_call(group_call, content)?;
                return (method == "group").then_some(Binding::GroupClosure(group_call, receiver));
            }
            _ => {}
        }
        current = parent;
    }
    None
}

fn binds_name(declaration: Node, name: &str, content: &str) -> bool {
    declaration
        .child_by_field_name("name")
        .and_then(|pattern| pattern.child_by_field_name("bound_identifier"))
        .and_then(|bound| content.get(bound.start_byte()..bound.end_byte()))
        == Some(name)
}

fn lambda_binds(lambda: Node, name: &str, content: &str) -> bool {
    let Some(parameters) = lambda
        .child_by_field_name("type")
        .and_then(|signature| named_child_of_kind(signature, "lambda_function_type_parameters"))
    else {
        return false;
    };
    let mut cursor = parameters.walk();
    parameters.named_children(&mut cursor).any(|parameter| {
        parameter
            .child_by_field_name("name")
            .and_then(|bound| content.get(bound.start_byte()..bound.end_byte()))
            == Some(name)
    })
}

fn walk_manifest(
    node: Node,
    language: &str,
    file_path: &str,
    content: &str,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "call_expression"
        && let Some(fact) = manifest_fact(node, language, file_path, content)
    {
        facts.push(fact);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_manifest(child, language, file_path, content, child_depth, facts);
    }
}

fn manifest_fact(
    call: Node,
    language: &str,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let callee = call.named_child(0)?;
    let callee_text = content.get(callee.start_byte()..callee.end_byte())?;
    let arguments = swift_call_arguments(call, content);
    let labeled = |label: &str| {
        arguments
            .iter()
            .find(|(name, _)| name.as_deref() == Some(label))
            .map(|(_, value)| *value)
    };
    let labeled_string =
        |label: &str| labeled(label).and_then(|value| swift_static_string(value, content));
    if callee_text == "Package" {
        let mut metadata = manifest_metadata();
        insert_string(&mut metadata, "name", labeled_string("name")?);
        return Some(fact_for_node(
            file_path,
            language,
            SWIFTPM_PACKAGE_PATTERN_ID,
            "package",
            call,
            metadata,
        ));
    }
    if callee.kind() != "prefix_expression" || !is_manifest_member(call, content) {
        return None;
    }
    let factory = callee_text.strip_prefix('.')?;
    let mut metadata = manifest_metadata();
    let (pattern_id, capture) = match factory {
        "library" | "executable" | "plugin" if in_argument(call, "products", content) => {
            insert_string(&mut metadata, "product_kind", factory);
            insert_string(&mut metadata, "name", labeled_string("name")?);
            let targets = labeled("targets")
                .map(|list| static_string_elements(list, content))
                .unwrap_or_default();
            if !targets.is_empty() {
                insert_string_array(&mut metadata, "targets", targets);
            }
            (SWIFTPM_PRODUCT_PATTERN_ID, "product")
        }
        "target" | "testTarget" | "executableTarget" | "macro" | "plugin" | "systemLibrary"
        | "binaryTarget"
            if in_argument(call, "targets", content) =>
        {
            insert_string(&mut metadata, "target_kind", factory);
            insert_string(&mut metadata, "name", labeled_string("name")?);
            if let Some(path) = labeled_string("path") {
                insert_string(&mut metadata, "path", path);
            }
            let dependencies = labeled("dependencies")
                .map(|list| target_dependency_names(list, content))
                .unwrap_or_default();
            if !dependencies.is_empty() {
                insert_string_array(&mut metadata, "dependencies", dependencies);
            }
            (SWIFTPM_TARGET_PATTERN_ID, "target")
        }
        "package" if in_argument(call, "dependencies", content) => {
            let location = labeled_string("url").or_else(|| labeled_string("path"))?;
            let name = labeled_string("name")
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let last = location
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or(location);
                    last.trim_end_matches(".git").to_string()
                });
            insert_string(&mut metadata, "ecosystem", "swiftpm");
            insert_string(&mut metadata, "name", &name);
            insert_string(&mut metadata, "group", "dependencies");
            insert_string(&mut metadata, "location", location);
            let requirement: Vec<&str> = arguments
                .iter()
                .filter(|(label, _)| !matches!(label.as_deref(), Some("url" | "path" | "name")))
                .filter_map(|(label, value)| {
                    let start = label
                        .as_ref()
                        .and_then(|_| value.parent())
                        .map_or(value.start_byte(), |argument| argument.start_byte());
                    content.get(start..value.end_byte())
                })
                .collect();
            if !requirement.is_empty() {
                insert_string(&mut metadata, "version", &requirement.join(", "));
            }
            (MANIFEST_DEPENDENCY_PATTERN_ID, "dependency")
        }
        _ => return None,
    };
    Some(fact_for_node(
        file_path, language, pattern_id, capture, call, metadata,
    ))
}

/// The call sits in the `<label>:` array argument of `Package(...)`.
fn in_argument(call: Node, label: &str, content: &str) -> bool {
    let Some(array) = call
        .parent()
        .filter(|parent| parent.kind() == "array_literal")
    else {
        return false;
    };
    let Some(argument) = array
        .parent()
        .filter(|parent| parent.kind() == "value_argument")
    else {
        return false;
    };
    argument
        .child_by_field_name("name")
        .and_then(|name| content.get(name.start_byte()..name.end_byte()))
        == Some(label)
        && package_call_of(argument).is_some()
}

fn package_call_of(argument: Node) -> Option<Node> {
    let call = argument.parent()?.parent()?.parent()?;
    (call.kind() == "call_expression").then_some(call)
}

fn is_manifest_member(call: Node, content: &str) -> bool {
    let Some(argument) = call
        .parent()
        .and_then(|array| array.parent())
        .filter(|parent| parent.kind() == "value_argument")
    else {
        return false;
    };
    package_call_of(argument)
        .and_then(|package| package.named_child(0))
        .and_then(|callee| content.get(callee.start_byte()..callee.end_byte()))
        == Some("Package")
}

fn static_string_elements(list: Node, content: &str) -> Vec<String> {
    if list.kind() != "array_literal" {
        return Vec::new();
    }
    let mut cursor = list.walk();
    list.named_children(&mut cursor)
        .filter_map(|element| swift_static_string(element, content))
        .map(str::to_string)
        .collect()
}

fn target_dependency_names(list: Node, content: &str) -> Vec<String> {
    if list.kind() != "array_literal" {
        return Vec::new();
    }
    let mut cursor = list.walk();
    list.named_children(&mut cursor)
        .filter_map(|element| {
            if let Some(name) = swift_static_string(element, content) {
                return Some(name.to_string());
            }
            if element.kind() != "call_expression" {
                return None;
            }
            swift_call_arguments(element, content)
                .into_iter()
                .find(|(label, _)| label.as_deref() == Some("name"))
                .and_then(|(_, value)| swift_static_string(value, content))
                .map(str::to_string)
        })
        .collect()
}

fn manifest_metadata() -> HashMap<String, Value> {
    HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String("dependencies".to_string()),
        ),
    ])
}

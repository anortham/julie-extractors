//! Dart framework facts: go_router route definitions and navigation
//! references, and shelf_router routes.
//!
//! Each family is gated on its package import. go_router definitions are
//! `GoRoute(path: '...')` calls; a `GoRoute` in the `routes:` list of another
//! joins its path to the parent's. shelf_router routes are `@Route.<verb>`
//! handler annotations and verb calls on a same-file `Router()`.
use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_node, insert_string, insert_string_array};
use super::scan::{RouteFactSpec, route_fact};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const GO_ROUTE_DEFINITION_PATTERN_ID: &str = "go_router.route_definition.v1";
const GO_ROUTE_REFERENCE_PATTERN_ID: &str = "go_router.route_reference.v1";
const SHELF_ROUTE_PATTERN_ID: &str = "shelf_router.route.v1";

const PATH_NAVIGATION: &[&str] = &["go", "push", "replace", "pushReplacement"];
const NAMED_NAVIGATION: &[&str] = &[
    "goNamed",
    "pushNamed",
    "replaceNamed",
    "pushReplacementNamed",
];
const HTTP_VERBS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("head", "HEAD"),
    ("options", "OPTIONS"),
];

pub(super) fn collect_dart_framework_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let root = tree.root_node();
    let imports = dart_imports(root, content);
    let go_router = imports
        .iter()
        .any(|import| import.uri.starts_with("package:go_router/"));
    let shelf_router = imports
        .iter()
        .any(|import| import.uri.starts_with("package:shelf_router/"));
    if !go_router && !shelf_router {
        return Vec::new();
    }
    let mut scan = Scan {
        language,
        tree,
        file_path,
        content,
        go_router,
        shelf_router,
        routers: if shelf_router {
            router_bindings(root, content)
        } else {
            HashSet::new()
        },
        route_paths: HashMap::new(),
        facts: Vec::new(),
    };
    scan.walk(root, 0);
    scan.facts
}

/// A Dart import directive: its URI and its `as` prefix.
pub(super) struct DartImport {
    pub uri: String,
    pub prefix: Option<String>,
}

/// The import directives of a Dart file.
pub(super) fn dart_imports(root: Node, content: &str) -> Vec<DartImport> {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|child| child.kind() == "import_or_export")
        .filter_map(|directive| {
            let text = content.get(directive.start_byte()..directive.end_byte())?;
            let start = text.find(['\'', '"'])? + 1;
            let end = start + text[start..].find(['\'', '"'])?;
            let mut words = text[end + 1..].split_whitespace();
            let prefix = words
                .by_ref()
                .find(|word| *word == "as")
                .and_then(|_| words.next())
                .map(|prefix| prefix.trim_end_matches(';').to_string());
            Some(DartImport {
                uri: text[start..end].to_string(),
                prefix,
            })
        })
        .collect()
}

/// The text of a single-part string literal with no interpolation.
pub(super) fn dart_static_string<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    if node.kind() != "string_literal" || node.named_child_count() != 1 {
        return None;
    }
    let part = node.named_child(0)?;
    let mut cursor = part.walk();
    if part
        .named_children(&mut cursor)
        .any(|child| child.kind() == "template_substitution")
    {
        return None;
    }
    let text = content.get(part.start_byte()..part.end_byte())?;
    let text = text.strip_prefix('r').unwrap_or(text);
    if text.starts_with("'''") || text.starts_with("\"\"\"") {
        return None;
    }
    text.strip_prefix(['\'', '"'])?.strip_suffix(['\'', '"'])
}

/// Positional arguments and `label: value` named arguments of a call.
pub(super) fn dart_arguments<'t>(
    arguments: Node<'t>,
    content: &str,
) -> (Vec<Node<'t>>, HashMap<String, Node<'t>>) {
    let mut positional = Vec::new();
    let mut named = HashMap::new();
    let mut cursor = arguments.walk();
    for argument in arguments.named_children(&mut cursor) {
        if argument.kind() == "named_argument" {
            let mut argument_cursor = argument.walk();
            let label = argument
                .named_children(&mut argument_cursor)
                .find(|child| child.kind() == "label")
                .and_then(|label| label.named_child(0))
                .and_then(|name| content.get(name.start_byte()..name.end_byte()));
            let mut value_cursor = argument.walk();
            let value = argument
                .named_children(&mut value_cursor)
                .filter(|child| child.kind() != "label")
                .last();
            if let (Some(label), Some(value)) = (label, value) {
                named.insert(label.to_string(), value);
            }
        } else {
            positional.push(argument);
        }
    }
    (positional, named)
}

/// The receiver and method name of `receiver.method(...)`, including a
/// generic `receiver.method<T>(...)`.
pub(super) fn dart_method_call<'t>(
    call: Node<'t>,
    content: &'t str,
) -> Option<(Node<'t>, &'t str)> {
    if call.kind() != "call_expression" {
        return None;
    }
    let mut function = call.child_by_field_name("function")?;
    if function.kind() == "instantiation_expression" {
        function = function.child_by_field_name("function")?;
    }
    if !matches!(
        function.kind(),
        "member_expression" | "null_aware_member_expression"
    ) {
        return None;
    }
    let object = function.child_by_field_name("object")?;
    let property = function.child_by_field_name("property")?;
    Some((
        object,
        content.get(property.start_byte()..property.end_byte())?,
    ))
}

/// Names bound to `Router()` anywhere in the file.
fn router_bindings(root: Node, content: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_router_bindings(root, content, 0, &mut names);
    names
}

fn collect_router_bindings(node: Node, content: &str, depth: u32, names: &mut HashSet<String>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(
        node.kind(),
        "static_final_declaration" | "initialized_variable_definition" | "initialized_identifier"
    ) && let (Some(name), Some(value)) = (
        node.child_by_field_name("name"),
        node.child_by_field_name("value"),
    ) && is_router_construction(value, content)
        && let Some(name) = content.get(name.start_byte()..name.end_byte())
    {
        names.insert(name.to_string());
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_router_bindings(child, content, child_depth, names);
    }
}

fn is_router_construction(node: Node, content: &str) -> bool {
    node.kind() == "call_expression"
        && node
            .child_by_field_name("function")
            .and_then(|function| content.get(function.start_byte()..function.end_byte()))
            == Some("Router")
}

struct Scan<'a> {
    language: &'a str,
    tree: &'a Tree,
    file_path: &'a str,
    content: &'a str,
    go_router: bool,
    shelf_router: bool,
    routers: HashSet<String>,
    /// Effective path of each `GoRoute(...)` call by start byte, so a nested
    /// route joins its parent's path.
    route_paths: HashMap<usize, String>,
    facts: Vec<StructuralFact>,
}

impl Scan<'_> {
    fn walk(&mut self, node: Node, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        match node.kind() {
            "call_expression" => {
                if self.go_router {
                    self.go_route_definition(node);
                    self.go_route_reference(node);
                }
                if self.shelf_router {
                    self.shelf_router_call(node, node);
                }
            }
            "cascade_call_expression" if self.shelf_router => {
                self.shelf_router_call(node, node);
            }
            "annotation" if self.shelf_router => self.shelf_route_annotation(node),
            _ => {}
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, child_depth);
        }
    }

    fn text(&self, node: Node) -> &str {
        self.content
            .get(node.start_byte()..node.end_byte())
            .unwrap_or("")
    }

    fn go_route_definition(&mut self, call: Node) {
        let Some(function) = call.child_by_field_name("function") else {
            return;
        };
        if self.text(function) != "GoRoute" {
            return;
        }
        let Some(arguments) = call.child_by_field_name("arguments") else {
            return;
        };
        let (_, named) = dart_arguments(arguments, self.content);
        let Some(route_path) = named
            .get("path")
            .and_then(|path| dart_static_string(*path, self.content))
        else {
            return;
        };
        let parent_path = enclosing_go_route(call, self.content)
            .and_then(|parent| self.route_paths.get(&parent.start_byte()).cloned());
        let effective = match &parent_path {
            Some(parent) if !route_path.starts_with('/') => {
                join_route_templates(parent, route_path)
            }
            _ => route_path.to_string(),
        };
        self.route_paths
            .insert(call.start_byte(), effective.clone());
        let mut metadata = base_metadata("frontend_navigation", "go_router");
        insert_string(&mut metadata, "route_path", route_path);
        if let Some(parent) = &parent_path {
            insert_string(&mut metadata, "parent_route_path", parent);
        }
        insert_string(&mut metadata, "effective_route_template", &effective);
        insert_normalized(&mut metadata, &effective, ParamFlavor::Colon);
        if let Some(name) = named
            .get("name")
            .and_then(|name| dart_static_string(*name, self.content))
        {
            insert_string(&mut metadata, "route_name", name);
        }
        if let Some(component) = named
            .get("builder")
            .or_else(|| named.get("pageBuilder"))
            .and_then(|builder| built_widget(*builder, self.content))
        {
            insert_string(&mut metadata, "route_component", component);
        }
        self.facts.push(fact_for_node(
            self.file_path,
            self.language,
            GO_ROUTE_DEFINITION_PATTERN_ID,
            "route_definition",
            call,
            metadata,
        ));
    }

    fn go_route_reference(&mut self, call: Node) {
        let Some((_, method)) = dart_method_call(call, self.content) else {
            return;
        };
        let is_path = PATH_NAVIGATION.contains(&method);
        if !is_path && !NAMED_NAVIGATION.contains(&method) {
            return;
        }
        let Some(target) = call
            .child_by_field_name("arguments")
            .and_then(|arguments| dart_arguments(arguments, self.content).0.first().copied())
            .and_then(|target| dart_static_string(target, self.content))
        else {
            return;
        };
        let mut metadata = base_metadata("frontend_navigation", "go_router");
        insert_string(&mut metadata, "navigation_method", method);
        if is_path {
            insert_string(&mut metadata, "target_path", target);
            let normalized = normalize_route_template(target, ParamFlavor::Colon);
            insert_string(
                &mut metadata,
                "normalized_route_template",
                &normalized.template,
            );
        } else {
            insert_string(&mut metadata, "route_name", target);
        }
        self.facts.push(fact_for_node(
            self.file_path,
            self.language,
            GO_ROUTE_REFERENCE_PATTERN_ID,
            "route_reference",
            call,
            metadata,
        ));
    }

    /// `router.get('/path', handler)` or `Router()..get('/path', handler)`.
    fn shelf_router_call(&mut self, call: Node, anchor: Node) {
        let (receiver, method, arguments) = if call.kind() == "cascade_call_expression" {
            let Some(receiver) = call.parent().and_then(cascade_target) else {
                return;
            };
            let Some(property) = call.child_by_field_name("property") else {
                return;
            };
            (
                receiver,
                self.text(property),
                call.child_by_field_name("arguments"),
            )
        } else {
            let Some((receiver, method)) = dart_method_call(call, self.content) else {
                return;
            };
            (receiver, method, call.child_by_field_name("arguments"))
        };
        let is_router = is_router_construction(receiver, self.content)
            || self.routers.contains(self.text(receiver));
        let Some(verb) = HTTP_VERBS
            .iter()
            .find(|(name, _)| *name == method)
            .map(|(_, verb)| *verb)
        else {
            return;
        };
        let Some(arguments) = arguments.filter(|_| is_router) else {
            return;
        };
        let (positional, _) = dart_arguments(arguments, self.content);
        let Some(path) = positional
            .first()
            .and_then(|path| dart_static_string(*path, self.content))
        else {
            return;
        };
        let handler = positional
            .get(1)
            .filter(|handler| matches!(handler.kind(), "identifier" | "member_expression"))
            .map(|handler| self.text(*handler).to_string());
        self.push_shelf_route(anchor, "router_call", path, verb, handler.as_deref());
    }

    fn shelf_route_annotation(&mut self, annotation: Node) {
        let Some(name) = annotation.child_by_field_name("name") else {
            return;
        };
        let name = self.text(name).to_string();
        let Some(arguments) = find_named_child(annotation, "annotation_arguments") else {
            return;
        };
        let strings: Vec<&str> = {
            let mut cursor = arguments.walk();
            arguments
                .named_children(&mut cursor)
                .filter(|child| child.kind() == "string_literal")
                .filter_map(|literal| dart_static_string(literal, self.content))
                .collect()
        };
        let (verb, path) = match name.strip_prefix("Route.") {
            Some(method) => {
                let Some(verb) = HTTP_VERBS
                    .iter()
                    .find(|(name, _)| *name == method)
                    .map(|(_, verb)| verb.to_string())
                else {
                    return;
                };
                let Some(path) = strings.first() else {
                    return;
                };
                (verb, *path)
            }
            None if name == "Route" && strings.len() == 2 => {
                (strings[0].to_uppercase(), strings[1])
            }
            None => return,
        };
        let handler = annotation
            .parent()
            .and_then(|member| member.child_by_field_name("signature"))
            .and_then(|signature| find_named_child(signature, "function_signature"))
            .and_then(|signature| signature.child_by_field_name("name"))
            .map(|name| self.text(name).to_string());
        let anchor = annotation.parent().unwrap_or(annotation);
        self.push_shelf_route(anchor, "annotation", path, &verb, handler.as_deref());
    }

    fn push_shelf_route(
        &mut self,
        anchor: Node,
        api_style: &str,
        path: &str,
        verb: &str,
        handler: Option<&str>,
    ) {
        let spec = RouteFactSpec {
            framework: "shelf_router",
            pattern_id: SHELF_ROUTE_PATTERN_ID,
            capture_name: "route",
            api_style,
            route_template: path,
            verb: Some(verb),
            verb_source: Some("attested"),
            flavor: ParamFlavor::AngleBrackets,
            prefix: None,
            prefix_key: None,
        };
        if let Some(fact) = route_fact(
            self.language,
            self.tree,
            self.file_path,
            self.content,
            anchor.start_byte(),
            anchor.end_byte(),
            spec,
            |metadata| {
                if let Some(handler) = handler {
                    insert_string(metadata, "handler", handler);
                }
            },
        ) {
            self.facts.push(fact);
        }
    }
}

fn insert_normalized(metadata: &mut HashMap<String, Value>, template: &str, flavor: ParamFlavor) {
    let normalized = normalize_route_template(template, flavor);
    insert_string(metadata, "normalized_route_template", &normalized.template);
    if !normalized.dynamic_segments.is_empty() {
        insert_string_array(metadata, "dynamic_segments", normalized.dynamic_segments);
    }
}

/// The object a `..member` cascade section applies to.
fn cascade_target(section: Node) -> Option<Node> {
    let mut current = section.prev_named_sibling();
    while let Some(sibling) = current {
        if sibling.kind() != "cascade_section" {
            return Some(sibling);
        }
        current = sibling.prev_named_sibling();
    }
    None
}

fn find_named_child<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

/// The `GoRoute(...)` whose `routes:` list holds this call.
fn enclosing_go_route<'t>(call: Node<'t>, content: &str) -> Option<Node<'t>> {
    let list = call
        .parent()
        .filter(|parent| parent.kind() == "list_literal")?;
    let argument = list
        .parent()
        .filter(|parent| parent.kind() == "named_argument")?;
    let parent_call = argument.parent()?.parent()?;
    let function = parent_call.child_by_field_name("function")?;
    (content.get(function.start_byte()..function.end_byte()) == Some("GoRoute"))
        .then_some(parent_call)
}

/// The widget class a `(context, state) => Widget(...)` builder constructs.
fn built_widget<'a>(builder: Node<'_>, content: &'a str) -> Option<&'a str> {
    let body = match builder.kind() {
        "function_expression" => builder.child_by_field_name("body")?.named_child(0)?,
        // `(c, s) => Page()` parses as a call of the closure `(c, s) => Page`.
        "call_expression" => {
            let closure = builder.child_by_field_name("function")?;
            let body = closure.child_by_field_name("body")?.named_child(0)?;
            return (body.kind() == "identifier")
                .then(|| content.get(body.start_byte()..body.end_byte()))
                .flatten();
        }
        _ => return None,
    };
    let type_node = match body.kind() {
        "const_object_expression" | "new_expression" => body.child_by_field_name("type")?,
        "call_expression" => body.child_by_field_name("function")?,
        _ => return None,
    };
    content.get(type_node.start_byte()..type_node.end_byte())
}

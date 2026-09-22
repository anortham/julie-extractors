use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_span, insert_string, insert_string_array};
use super::scan::parse_ruby_string_literal;
use super::{RAILS_MOUNT_PATTERN_ID, RAILS_RESOURCE_ROUTE_PATTERN_ID, RAILS_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const HTTP_VERBS: [(&str, &str); 5] = [
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
];

/// Paths of one `resources`/`resource` declaration, already joined with the
/// enclosing scope.
#[derive(Clone)]
struct ResourcePaths {
    controller: String,
    collection: String,
    member: String,
    nested: String,
}

/// Routing state at one point of the routes DSL. `prefix` is `None` when an
/// enclosing scope path is dynamic, which silences every nested route.
#[derive(Clone)]
struct RouteScope {
    prefix: Option<String>,
    resource: Option<ResourcePaths>,
}

impl RouteScope {
    fn root() -> Self {
        Self {
            prefix: Some(String::new()),
            resource: None,
        }
    }

    fn with_prefix(&self, path: Option<String>) -> Self {
        let prefix = match (&self.prefix, path) {
            (Some(prefix), Some(path)) => Some(join_scope(prefix, &path)),
            _ => None,
        };
        Self {
            prefix,
            resource: self.resource.clone(),
        }
    }
}

struct RouteCollector<'a> {
    language: &'a str,
    file_path: &'a str,
    content: &'a str,
    facts: Vec<StructuralFact>,
}

pub(super) fn collect_rails_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    // Rails 6.1+ split route files (`config/routes/*.rb`, loaded via
    // `draw :name`) hold top-level DSL; everything else requires the DSL to
    // sit inside a `routes.draw do ... end` block.
    let split_route_file = file_path.replace('\\', "/").contains("config/routes/");
    if !split_route_file && !content.contains(".routes.draw") {
        return Vec::new();
    }
    let mut collector = RouteCollector {
        language,
        file_path,
        content,
        facts: Vec::new(),
    };
    let root = tree.root_node();
    if split_route_file {
        collector.walk_children(root, &RouteScope::root(), 0);
    } else {
        collector.find_draw_blocks(root, 0);
    }
    collector.facts
}

impl<'a> RouteCollector<'a> {
    fn text(&self, node: Node<'a>) -> &'a str {
        &self.content[node.byte_range()]
    }

    fn find_draw_blocks(&mut self, node: Node<'a>, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        if node.kind() == "call"
            && self.method_name(node) == Some("draw")
            && node
                .child_by_field_name("receiver")
                .is_some_and(|receiver| self.method_name(receiver) == Some("routes"))
            && let Some(block) = node.child_by_field_name("block")
        {
            self.walk_children(block, &RouteScope::root(), depth);
            return;
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.find_draw_blocks(child, child_depth);
        }
    }

    fn walk_children(&mut self, node: Node<'a>, scope: &RouteScope, depth: u32) {
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child, scope, child_depth);
        }
    }

    fn walk(&mut self, node: Node<'a>, scope: &RouteScope, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        match node.kind() {
            "comment" | "string" | "heredoc_body" | "method" | "singleton_method" | "class"
            | "module" => {}
            "call" if node.child_by_field_name("receiver").is_none() => {
                self.visit_dsl_call(node, scope, depth);
            }
            _ => self.walk_children(node, scope, depth),
        }
    }

    fn walk_block(&mut self, call: Node<'a>, scope: &RouteScope, depth: u32) {
        if let Some(block) = call.child_by_field_name("block") {
            self.walk_children(block, scope, depth);
        }
    }

    fn method_name(&self, call: Node<'a>) -> Option<&'a str> {
        (call.kind() == "call")
            .then(|| call.child_by_field_name("method"))
            .flatten()
            .map(|method| self.text(method))
    }

    fn visit_dsl_call(&mut self, call: Node<'a>, scope: &RouteScope, depth: u32) {
        let Some(method) = self.method_name(call) else {
            return;
        };
        let args = RouteArguments::from_call(self, call);
        match method {
            "namespace" => {
                let path = args
                    .positional(0)
                    .and_then(|name| static_value(self.content, name))
                    .map(|name| args.value_of("path").unwrap_or(name));
                self.walk_block(call, &scope.with_prefix(path), depth);
            }
            "scope" => {
                let path = match args.pair("path").or_else(|| args.positional(0)) {
                    Some(node) => static_value(self.content, node),
                    None => Some(String::new()),
                };
                self.walk_block(call, &scope.with_prefix(path), depth);
            }
            "resources" | "resource" => self.visit_resources(call, &args, scope, depth),
            "member" | "collection" => {
                let inner = match &scope.resource {
                    Some(resource) => {
                        let path = if method == "member" {
                            &resource.member
                        } else {
                            &resource.collection
                        };
                        RouteScope {
                            prefix: scope.prefix.as_ref().map(|_| path.clone()),
                            resource: Some(resource.clone()),
                        }
                    }
                    None => scope.clone(),
                };
                self.walk_block(call, &inner, depth);
            }
            "root" => {
                let target = args
                    .pair("to")
                    .or_else(|| args.positional(0))
                    .and_then(|node| static_value(self.content, node));
                self.push_route(call, &args, scope, Some("GET"), "/", target);
            }
            "match" => {
                let Some((path, target)) = args.route_path(self) else {
                    return;
                };
                let verbs = match args.pair("via") {
                    Some(via) if static_value(self.content, via).as_deref() == Some("all") => {
                        vec![None]
                    }
                    Some(via) => static_values(self.content, via)
                        .into_iter()
                        .map(|verb| Some(verb.to_uppercase()))
                        .collect(),
                    None => return,
                };
                for verb in verbs {
                    let verb = verb.as_deref();
                    self.push_path_route(call, &args, scope, verb, &path, target.clone());
                }
            }
            "mount" => self.push_mount(call, &args, scope),
            _ => {
                if let Some((_, verb)) = HTTP_VERBS.iter().find(|(name, _)| *name == method) {
                    if let Some((path, target)) = args.route_path(self) {
                        self.push_path_route(call, &args, scope, Some(verb), &path, target);
                    }
                } else {
                    self.walk_block(call, scope, depth);
                }
            }
        }
    }

    fn visit_resources(
        &mut self,
        call: Node<'a>,
        args: &RouteArguments<'a>,
        scope: &RouteScope,
        depth: u32,
    ) {
        let singular = self.method_name(call) == Some("resource");
        let kind = if singular { "singular" } else { "collection" };
        let parent = scope.prefix.clone();
        for name_node in args.positionals.iter().copied() {
            let Some(name) = static_value(self.content, name_node) else {
                continue;
            };
            let Some(parent) = &parent else {
                continue;
            };
            self.push_resource(call, args, parent, kind, &name);
            let resource_path = args.value_of("path").unwrap_or_else(|| name.clone());
            let collection = join_scope(parent, &resource_path);
            let (member, nested) = if singular {
                (collection.clone(), collection.clone())
            } else {
                (
                    format!("{collection}/:id"),
                    format!("{collection}/:{}_id", singular_resource_name(&name)),
                )
            };
            let controller = args.value_of("controller").unwrap_or_else(|| {
                if singular {
                    plural_resource_name(&name)
                } else {
                    name.clone()
                }
            });
            let inner = RouteScope {
                prefix: Some(nested.clone()),
                resource: Some(ResourcePaths {
                    controller,
                    collection,
                    member,
                    nested,
                }),
            };
            self.walk_block(call, &inner, depth);
        }
    }

    fn push_path_route(
        &mut self,
        call: Node<'a>,
        args: &RouteArguments<'a>,
        scope: &RouteScope,
        verb: Option<&str>,
        path: &str,
        target: Option<String>,
    ) {
        let placed = match (args.value_of("on").as_deref(), &scope.resource) {
            (Some("member"), Some(resource)) => RouteScope {
                prefix: scope.prefix.as_ref().map(|_| resource.member.clone()),
                resource: scope.resource.clone(),
            },
            (Some("collection"), Some(resource)) => RouteScope {
                prefix: scope.prefix.as_ref().map(|_| resource.collection.clone()),
                resource: scope.resource.clone(),
            },
            _ => scope.clone(),
        };
        let target = target.or_else(|| {
            let resource = placed.resource.as_ref()?;
            let action = args.value_of("action").unwrap_or_else(|| path.to_string());
            action
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                .then(|| format!("{}#{action}", resource.controller))
        });
        self.push_route(call, args, &placed, verb, path, target);
    }

    fn push_route(
        &mut self,
        call: Node<'a>,
        args: &RouteArguments<'a>,
        scope: &RouteScope,
        verb: Option<&str>,
        route_template: &str,
        controller_action: Option<String>,
    ) {
        let Some(scope_path) = scope.prefix.as_deref() else {
            return;
        };
        let Some(span) = self.dsl_span(call) else {
            return;
        };
        let mut metadata = base_metadata("framework", "rails");
        insert_string(&mut metadata, "api_style", "dsl_routing");
        insert_string(&mut metadata, "route_template", route_template);
        let normalized_source = if scope_path.is_empty() {
            route_template.to_string()
        } else {
            insert_string(&mut metadata, "scope_path", scope_path);
            let effective = join_route_templates(scope_path, route_template);
            insert_string(&mut metadata, "effective_route_template", &effective);
            effective
        };
        let normalized = normalize_route_template(&normalized_source, ParamFlavor::Colon);
        insert_string(
            &mut metadata,
            "normalized_route_template",
            &normalized.template,
        );
        if !normalized.dynamic_segments.is_empty() {
            insert_string_array(
                &mut metadata,
                "dynamic_segments",
                normalized.dynamic_segments,
            );
        }
        if let Some(verb) = verb {
            insert_string(&mut metadata, "verb", verb);
            insert_string(&mut metadata, "verb_source", "attested");
        }
        if let Some(controller_action) = controller_action.filter(|value| value.contains('#')) {
            insert_string(&mut metadata, "controller_action", &controller_action);
        }
        if let Some(route_name) = args.value_of("as") {
            insert_string(&mut metadata, "route_name", &route_name);
        }
        self.facts.push(fact_for_span(
            self.file_path,
            self.language,
            RAILS_ROUTE_PATTERN_ID,
            "route",
            call.kind(),
            span,
            metadata,
        ));
    }

    fn push_resource(
        &mut self,
        call: Node<'a>,
        args: &RouteArguments<'a>,
        scope_path: &str,
        kind: &str,
        resource_name: &str,
    ) {
        let Some(span) = self.dsl_span(call) else {
            return;
        };
        let mut metadata = base_metadata("framework", "rails");
        insert_string(&mut metadata, "api_style", "dsl_routing");
        insert_string(&mut metadata, "resource_kind", kind);
        insert_string(&mut metadata, "resource_name", resource_name);
        if !scope_path.is_empty() {
            insert_string(&mut metadata, "scope_path", scope_path);
        }
        for key in ["only", "except"] {
            if let Some(actions) = args.pair(key) {
                insert_string_array(&mut metadata, key, static_values(self.content, actions));
            }
        }
        self.facts.push(fact_for_span(
            self.file_path,
            self.language,
            RAILS_RESOURCE_ROUTE_PATTERN_ID,
            "resource_route",
            call.kind(),
            span,
            metadata,
        ));
    }

    fn push_mount(&mut self, call: Node<'a>, args: &RouteArguments<'a>, scope: &RouteScope) {
        let Some(scope_path) = scope.prefix.as_deref() else {
            return;
        };
        let (target, mount_path) = if let Some((key, value)) = args.string_keyed_pair {
            (key, static_value(self.content, value))
        } else if let Some(target) = args.positional(0) {
            (target, args.value_of("at"))
        } else {
            return;
        };
        let Some(mount_path) = mount_path else {
            return;
        };
        let Some(span) = self.dsl_span(call) else {
            return;
        };
        let mut metadata = base_metadata("framework", "rails");
        insert_string(&mut metadata, "mount_target", self.text(target));
        let full_mount_path = if scope_path.is_empty() {
            mount_path.clone()
        } else {
            insert_string(&mut metadata, "scope_path", scope_path);
            join_route_templates(scope_path, &mount_path)
        };
        insert_string(&mut metadata, "mount_path", &mount_path);
        let normalized = normalize_route_template(&full_mount_path, ParamFlavor::Colon);
        insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
        self.facts.push(fact_for_span(
            self.file_path,
            self.language,
            RAILS_MOUNT_PATTERN_ID,
            "mount",
            call.kind(),
            span,
            metadata,
        ));
    }

    /// The DSL call without its block, so one fact covers `resources :posts`
    /// and not every nested route.
    fn dsl_span(&self, call: Node<'a>) -> Option<NormalizedSpan> {
        let end = call
            .child_by_field_name("arguments")
            .or_else(|| call.child_by_field_name("method"))?
            .end_byte();
        let end = if self.content.as_bytes().get(end) == Some(&b')') {
            end + 1
        } else {
            end
        };
        NormalizedSpan::from_content_range(self.content, call.start_byte(), end)
    }
}

/// The argument list of one DSL call, split into positional values and
/// keyword pairs.
struct RouteArguments<'a> {
    content: &'a str,
    positionals: Vec<Node<'a>>,
    pairs: Vec<(String, Node<'a>)>,
    string_keyed_pair: Option<(Node<'a>, Node<'a>)>,
}

impl<'a> RouteArguments<'a> {
    fn from_call(collector: &RouteCollector<'a>, call: Node<'a>) -> Self {
        let mut arguments = Self {
            content: collector.content,
            positionals: Vec::new(),
            pairs: Vec::new(),
            string_keyed_pair: None,
        };
        let Some(list) = call.child_by_field_name("arguments") else {
            return arguments;
        };
        let mut cursor = list.walk();
        for child in list.named_children(&mut cursor) {
            match child.kind() {
                "pair" => arguments.push_pair(child),
                "hash" => {
                    let mut hash_cursor = child.walk();
                    for pair in child.named_children(&mut hash_cursor) {
                        if pair.kind() == "pair" {
                            arguments.push_pair(pair);
                        }
                    }
                }
                "comment" => {}
                _ => arguments.positionals.push(child),
            }
        }
        arguments
    }

    fn push_pair(&mut self, pair: Node<'a>) {
        let (Some(key), Some(value)) = (
            pair.child_by_field_name("key"),
            pair.child_by_field_name("value"),
        ) else {
            return;
        };
        match key.kind() {
            "hash_key_symbol" | "simple_symbol" => {
                let name = self.content[key.byte_range()].trim_start_matches(':');
                self.pairs.push((name.to_string(), value));
            }
            _ if self.string_keyed_pair.is_none() => self.string_keyed_pair = Some((key, value)),
            _ => {}
        }
    }

    fn positional(&self, index: usize) -> Option<Node<'a>> {
        self.positionals.get(index).copied()
    }

    fn pair(&self, key: &str) -> Option<Node<'a>> {
        self.pairs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| *value)
    }

    fn value_of(&self, key: &str) -> Option<String> {
        static_value(self.content, self.pair(key)?)
    }

    /// The route path and any literal `controller#action` target, from
    /// `get "path", to: "c#a"`, `get :path`, or `get "path" => "c#a"`.
    fn route_path(&self, collector: &RouteCollector<'a>) -> Option<(String, Option<String>)> {
        let target = self.value_of("to").or_else(|| {
            let controller = self.value_of("controller")?;
            let action = self.value_of("action")?;
            Some(format!("{controller}#{action}"))
        });
        if let Some(path) = self.positional(0) {
            return Some((static_value(collector.content, path)?, target));
        }
        let (key, value) = self.string_keyed_pair?;
        let path = static_value(collector.content, key)?;
        Some((path, static_value(collector.content, value).or(target)))
    }
}

fn join_scope(prefix: &str, path: &str) -> String {
    if path.is_empty() {
        return prefix.to_string();
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    if prefix.is_empty() {
        path
    } else {
        join_route_templates(prefix, &path)
    }
}

/// The literal text of a string, symbol, or bare word node. Interpolated or
/// computed values return `None`.
fn static_value(content: &str, node: Node) -> Option<String> {
    let text = &content[node.byte_range()];
    match node.kind() {
        "string" => {
            let (value, _) = parse_ruby_string_literal(text, 0)?;
            (!value.contains("#{")).then_some(value)
        }
        "simple_symbol" => Some(text.trim_start_matches(':').to_string()),
        "delimited_symbol" => {
            let (value, _) = parse_ruby_string_literal(text.strip_prefix(':')?, 0)?;
            (!value.contains("#{")).then_some(value)
        }
        "bare_symbol" | "bare_string" => {
            (node.named_child_count() <= 1 && !text.contains("#{")).then(|| text.to_string())
        }
        _ => None,
    }
}

/// Every literal element of an array, `%i[]`/`%w[]` list, or single value.
fn static_values(content: &str, node: Node) -> Vec<String> {
    match node.kind() {
        "array" | "symbol_array" | "string_array" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .filter_map(|element| static_value(content, element))
                .collect()
        }
        _ => static_value(content, node).into_iter().collect(),
    }
}

fn singular_resource_name(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("ies") {
        return format!("{stem}y");
    }
    name.strip_suffix('s').unwrap_or(name).to_string()
}

fn plural_resource_name(name: &str) -> String {
    if let Some(stem) = name.strip_suffix('y')
        && !stem.ends_with(['a', 'e', 'i', 'o', 'u'])
    {
        return format!("{stem}ies");
    }
    if name.ends_with('s') {
        return name.to_string();
    }
    format!("{name}s")
}

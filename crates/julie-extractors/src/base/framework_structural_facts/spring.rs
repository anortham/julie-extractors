//! Java Spring MVC annotation-controller route facts.
//!
//! The collector reads the tree: each type declaration owns its own class-level
//! `@RequestMapping` prefix, and each mapping annotation in a method's
//! `modifiers` produces one fact per template and verb. A nested type starts a
//! new prefix scope, so a DTO nested in a controller cannot reset or leak the
//! controller prefix. `@FeignClient` interfaces declare outbound calls, not
//! server routes, so they emit nothing here (the HTTP client collector reads
//! them).
//!
//! Static-literal silence: a route argument is used only when it is a plain
//! string literal (or an array of them); a dynamic route argument emits nothing.
//!
//! Each fact spans its declaration after the `modifiers` node, which is where
//! the Kotlin collector anchors its handler facts too.

use tree_sitter::{Node, Tree};

use super::SPRING_REQUEST_MAPPING_PATTERN_ID;
use super::helpers::{
    base_metadata, fact_for_span, insert_string, insert_string_array, node_text,
    smallest_node_covering_range,
};
use super::static_arg::{StaticArgLang, static_route_arg};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const IMPORT_NEEDLE: &str = "org.springframework.web.bind.annotation";

const REQUEST_METHODS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE",
];

const TYPE_DECLARATIONS: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
];

pub(super) fn collect_spring_request_mappings(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains(IMPORT_NEEDLE) {
        return Vec::new();
    }
    let mut facts = Vec::new();
    for declaration in java_type_declarations(tree.root_node()) {
        let annotations = java_annotations(declaration, content);
        if annotations
            .iter()
            .any(|annotation| annotation.name == "FeignClient")
        {
            continue;
        }
        let Some(body) = declaration.child_by_field_name("body") else {
            continue;
        };
        let class_mapping = annotations
            .iter()
            .find(|annotation| annotation.name == "RequestMapping")
            .map(|annotation| MappingArguments::parse(annotation.node, content));
        let prefixes = class_mapping
            .as_ref()
            .map(|mapping| mapping.templates.clone())
            .unwrap_or_default();
        for prefix in &prefixes {
            let spec = MappingFact {
                attribute_kind: "class_route",
                route_template: prefix,
                class_route_template: None,
                verb: None,
            };
            facts.extend(mapping_fact(
                language,
                tree,
                file_path,
                content,
                declaration,
                &spec,
            ));
        }
        for member in body.named_children(&mut body.walk()) {
            if member.kind() != "method_declaration" {
                continue;
            }
            for annotation in java_annotations(member, content) {
                emit_method_routes(
                    &annotation,
                    member,
                    &prefixes,
                    language,
                    tree,
                    file_path,
                    content,
                    &mut facts,
                );
            }
        }
    }
    facts
}

/// The default verb and attribute kind of a Spring mapping annotation.
pub(super) fn mapping_annotation_kind(name: &str) -> Option<(Option<&'static str>, &'static str)> {
    match name {
        "GetMapping" => Some((Some("GET"), "http_method")),
        "PostMapping" => Some((Some("POST"), "http_method")),
        "PutMapping" => Some((Some("PUT"), "http_method")),
        "PatchMapping" => Some((Some("PATCH"), "http_method")),
        "DeleteMapping" => Some((Some("DELETE"), "http_method")),
        "RequestMapping" => Some((None, "request_mapping")),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_method_routes(
    annotation: &JavaAnnotation,
    method: Node,
    prefixes: &[String],
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some((default_verb, attribute_kind)) = mapping_annotation_kind(annotation.name) else {
        return;
    };
    let arguments = MappingArguments::parse(annotation.node, content);
    let templates = if arguments.templates.is_empty() {
        if arguments.had_route_argument {
            return;
        }
        vec![String::new()]
    } else {
        arguments.templates
    };
    let verbs: Vec<Option<String>> = match default_verb {
        Some(verb) => vec![Some(verb.to_string())],
        None if arguments.verbs.is_empty() => vec![None],
        None => arguments.verbs.into_iter().map(Some).collect(),
    };
    let class_templates: Vec<Option<&str>> = if prefixes.is_empty() {
        vec![None]
    } else {
        prefixes
            .iter()
            .map(|prefix| Some(prefix.as_str()))
            .collect()
    };
    for class_template in &class_templates {
        for template in &templates {
            for verb in &verbs {
                let spec = MappingFact {
                    attribute_kind,
                    route_template: template,
                    class_route_template: *class_template,
                    verb: verb.as_deref(),
                };
                facts.extend(mapping_fact(
                    language, tree, file_path, content, method, &spec,
                ));
            }
        }
    }
}

/// Every Java type declaration in the file, outermost first.
pub(super) fn java_type_declarations(root: Node) -> Vec<Node> {
    let mut declarations = Vec::new();
    collect_type_declarations(root, 0, &mut declarations);
    declarations
}

fn collect_type_declarations<'tree>(node: Node<'tree>, depth: u32, out: &mut Vec<Node<'tree>>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if TYPE_DECLARATIONS.contains(&node.kind()) {
        out.push(node);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.named_children(&mut node.walk()) {
        collect_type_declarations(child, child_depth, out);
    }
}

/// One annotation in a declaration's `modifiers`, keyed by the simple name so
/// `@org.springframework…GetMapping` and `@GetMapping` read the same.
pub(super) struct JavaAnnotation<'tree, 'src> {
    pub(super) name: &'src str,
    pub(super) node: Node<'tree>,
}

pub(super) fn java_annotations<'tree, 'src>(
    declaration: Node<'tree>,
    content: &'src str,
) -> Vec<JavaAnnotation<'tree, 'src>> {
    let Some(modifiers) = declaration
        .children(&mut declaration.walk())
        .find(|child| child.kind() == "modifiers")
    else {
        return Vec::new();
    };
    modifiers
        .children(&mut modifiers.walk())
        .filter(|child| matches!(child.kind(), "annotation" | "marker_annotation"))
        .filter_map(|annotation| {
            let name = annotation.child_by_field_name("name")?;
            let terminal = match name.kind() {
                "scoped_identifier" => name.child_by_field_name("name")?,
                _ => name,
            };
            Some(JavaAnnotation {
                name: node_text(content, terminal)?,
                node: annotation,
            })
        })
        .collect()
}

/// The `(name, value)` elements of an annotation: `None` names the positional
/// value. A marker annotation has none.
pub(super) fn annotation_elements<'tree>(
    annotation: Node<'tree>,
    content: &str,
) -> Vec<(Option<String>, Node<'tree>)> {
    let Some(arguments) = annotation.child_by_field_name("arguments") else {
        return Vec::new();
    };
    arguments
        .named_children(&mut arguments.walk())
        .filter_map(|argument| {
            if argument.kind() != "element_value_pair" {
                return Some((None, argument));
            }
            let key = argument.child_by_field_name("key")?;
            let value = argument.child_by_field_name("value")?;
            Some((node_text(content, key).map(str::to_string), value))
        })
        .collect()
}

/// Static templates of an element value: one string literal, or the static
/// elements of an array. A dynamic array element is skipped, never guessed.
/// `None` when a single value is not a plain string literal.
pub(super) fn static_templates(value: Node, content: &str) -> Option<Vec<String>> {
    if value.kind() == "element_value_array_initializer" {
        return Some(
            value
                .named_children(&mut value.walk())
                .filter_map(|element| {
                    static_route_arg(element, content, StaticArgLang::Java).map(str::to_string)
                })
                .collect(),
        );
    }
    static_route_arg(value, content, StaticArgLang::Java).map(|template| vec![template.to_string()])
}

#[derive(Default)]
pub(super) struct MappingArguments {
    pub(super) templates: Vec<String>,
    pub(super) had_route_argument: bool,
    pub(super) verbs: Vec<String>,
}

impl MappingArguments {
    /// Route templates come only from the positional value or the `value` /
    /// `path` elements; `produces`/`consumes`/`params`/`headers` are not routes.
    pub(super) fn parse(annotation: Node, content: &str) -> Self {
        let mut arguments = Self::default();
        for (name, value) in annotation_elements(annotation, content) {
            match name.as_deref() {
                None | Some("value") | Some("path") => {
                    arguments.had_route_argument = true;
                    if let Some(mut templates) = static_templates(value, content) {
                        arguments.templates.append(&mut templates);
                    }
                }
                Some("method") => collect_request_methods(value, content, &mut arguments.verbs),
                _ => {}
            }
        }
        arguments
    }
}

fn collect_request_methods(value: Node, content: &str, verbs: &mut Vec<String>) {
    let values: Vec<Node> = if value.kind() == "element_value_array_initializer" {
        value.named_children(&mut value.walk()).collect()
    } else {
        vec![value]
    };
    for value in values {
        let terminal = match value.kind() {
            "field_access" => value.child_by_field_name("field"),
            "identifier" => Some(value),
            _ => None,
        };
        if let Some(verb) = terminal.and_then(|terminal| node_text(content, terminal))
            && REQUEST_METHODS.contains(&verb)
        {
            verbs.push(verb.to_string());
        }
    }
}

/// Join a class prefix and a method sub-path. An empty method path resolves to
/// the prefix alone, with no trailing slash.
pub(super) fn join_prefix(prefix: &str, template: &str) -> String {
    if template.is_empty() {
        prefix.to_string()
    } else {
        join_route_templates(prefix, template)
    }
}

/// The span of a declaration after its `modifiers`.
pub(super) fn declaration_span(declaration: Node) -> (usize, usize) {
    let end = declaration.end_byte();
    declaration
        .children(&mut declaration.walk())
        .find(|child| child.kind() != "modifiers")
        .map(|child| (child.start_byte(), end))
        .unwrap_or((declaration.start_byte(), end))
}

struct MappingFact<'a> {
    attribute_kind: &'static str,
    route_template: &'a str,
    class_route_template: Option<&'a str>,
    verb: Option<&'a str>,
}

fn mapping_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    declaration: Node,
    spec: &MappingFact,
) -> Option<StructuralFact> {
    let (start, end) = declaration_span(declaration);
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let effective = spec
        .class_route_template
        .map(|prefix| join_prefix(prefix, spec.route_template));
    let normalized = normalize_route_template(
        effective.as_deref().unwrap_or(spec.route_template),
        ParamFlavor::Braces,
    );
    let mut metadata = base_metadata("framework", "spring");
    insert_string(&mut metadata, "api_style", "annotation_routing");
    insert_string(&mut metadata, "attribute_kind", spec.attribute_kind);
    insert_string(&mut metadata, "route_template", spec.route_template);
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
    if let Some(class_route_template) = spec.class_route_template {
        insert_string(&mut metadata, "class_route_template", class_route_template);
    }
    if let Some(effective) = &effective {
        insert_string(&mut metadata, "effective_route_template", effective);
    }
    if let Some(verb) = spec.verb {
        insert_string(&mut metadata, "verb", verb);
        insert_string(&mut metadata, "verb_source", "attested");
    }
    Some(fact_for_span(
        file_path,
        language,
        SPRING_REQUEST_MAPPING_PATTERN_ID,
        "request_mapping",
        node.kind(),
        span,
        metadata,
    ))
}

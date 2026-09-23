//! JAX-RS / Jakarta REST resource route facts (Jersey, RESTEasy, Quarkus,
//! Dropwizard, Helidon, Jakarta EE).
//!
//! A resource class carries `@Path("/prefix")`; each resource method carries an
//! HTTP method designator (`@GET`, `@POST`, …) and an optional `@Path`. A method
//! with `@Path` and no designator is a sub-resource locator: it routes a prefix
//! to another resource class and has no verb. Import-gated on `javax.ws.rs` or
//! `jakarta.ws.rs`. Route arguments must be static string literals.

use tree_sitter::{Node, Tree};

use super::JAXRS_ROUTE_PATTERN_ID;
use super::helpers::{
    base_metadata, fact_for_span, insert_string, insert_string_array, smallest_node_covering_range,
};
use super::spring::{
    JavaAnnotation, annotation_elements, declaration_span, java_annotations,
    java_type_declarations, join_prefix, static_templates,
};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

const HTTP_METHOD_DESIGNATORS: &[&str] =
    &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

pub(super) fn collect_jaxrs_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains("javax.ws.rs") && !content.contains("jakarta.ws.rs") {
        return Vec::new();
    }
    let mut facts = Vec::new();
    for declaration in java_type_declarations(tree.root_node()) {
        let Some(body) = declaration.child_by_field_name("body") else {
            continue;
        };
        let class_paths = path_templates(&java_annotations(declaration, content), content);
        let prefix = match class_paths {
            Some(PathValue::Static(prefix)) => {
                let spec = RouteFact {
                    attribute_kind: "class_route",
                    route_template: &prefix,
                    class_route_template: None,
                    verb: None,
                };
                facts.extend(route_fact(
                    language,
                    tree,
                    file_path,
                    content,
                    declaration,
                    &spec,
                ));
                Some(prefix)
            }
            Some(PathValue::Dynamic) => continue,
            None => None,
        };
        for member in body.named_children(&mut body.walk()) {
            if member.kind() != "method_declaration" {
                continue;
            }
            let annotations = java_annotations(member, content);
            let verb = annotations
                .iter()
                .find(|annotation| HTTP_METHOD_DESIGNATORS.contains(&annotation.name))
                .map(|annotation| annotation.name);
            let template = match path_templates(&annotations, content) {
                Some(PathValue::Static(template)) => template,
                Some(PathValue::Dynamic) => continue,
                None if verb.is_some() => String::new(),
                None => continue,
            };
            let attribute_kind = if verb.is_some() {
                "resource_method"
            } else {
                "subresource_locator"
            };
            let spec = RouteFact {
                attribute_kind,
                route_template: &template,
                class_route_template: prefix.as_deref(),
                verb,
            };
            facts.extend(route_fact(
                language, tree, file_path, content, member, &spec,
            ));
        }
    }
    facts
}

enum PathValue {
    Static(String),
    Dynamic,
}

/// The template of the `@Path` annotation, if present.
fn path_templates(annotations: &[JavaAnnotation], content: &str) -> Option<PathValue> {
    let path = annotations
        .iter()
        .find(|annotation| annotation.name == "Path")?;
    let value = annotation_elements(path.node, content)
        .into_iter()
        .find(|(name, _)| name.as_deref().is_none_or(|name| name == "value"))
        .map(|(_, value)| value);
    Some(
        match value
            .and_then(|value| static_templates(value, content))
            .as_deref()
        {
            Some([template]) => PathValue::Static(template.clone()),
            _ => PathValue::Dynamic,
        },
    )
}

struct RouteFact<'a> {
    attribute_kind: &'static str,
    route_template: &'a str,
    class_route_template: Option<&'a str>,
    verb: Option<&'a str>,
}

fn route_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    declaration: Node,
    spec: &RouteFact,
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
    let mut metadata = base_metadata("framework", "jaxrs");
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
        JAXRS_ROUTE_PATTERN_ID,
        "route",
        node.kind(),
        span,
        metadata,
    ))
}

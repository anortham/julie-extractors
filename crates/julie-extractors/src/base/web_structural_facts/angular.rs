//! Angular Router route definitions: each route object in a `Routes` array
//! or in the array passed to `RouterModule.forRoot/forChild` or
//! `provideRouter`.

use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::ANGULAR_ROUTE_DEFINITION_PATTERN_ID;
use super::fact_builders::{base_metadata, fact_for_node, insert_string};
use super::js_imports::JsImportIndex;
use super::js_object_scan::join_frontend_route_paths;
use super::programmatic_navigation::{collect_calls, static_string_value};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn collect_angular_route_definitions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    if language != "typescript" || imports.angular_router.is_empty() {
        return Vec::new();
    }
    let mut declarators = Vec::new();
    collect_kind(tree.root_node(), "variable_declarator", &mut declarators, 0);
    let arrays_by_name: HashMap<&str, Node> = declarators
        .iter()
        .filter_map(|declarator| {
            let name = declarator
                .child_by_field_name("name")
                .filter(|name| name.kind() == "identifier")?;
            let value = declarator
                .child_by_field_name("value")
                .filter(|value| value.kind() == "array")?;
            Some((content.get(name.byte_range())?, value))
        })
        .collect();

    let mut roots: Vec<Node> = declarators
        .iter()
        .filter(|declarator| is_routes_annotation(**declarator, content, imports))
        .filter_map(|declarator| declarator.child_by_field_name("value"))
        .filter(|value| value.kind() == "array")
        .collect();
    let mut calls = Vec::new();
    collect_calls(tree.root_node(), &mut calls, 0);
    for call in calls {
        if !is_router_registration(call, content, imports) {
            continue;
        }
        let Some(arguments) = call.child_by_field_name("arguments") else {
            continue;
        };
        let mut cursor = arguments.walk();
        let Some(argument) = arguments.named_children(&mut cursor).next() else {
            continue;
        };
        match argument.kind() {
            "array" => roots.push(argument),
            "identifier" => {
                if let Some(array) = content
                    .get(argument.byte_range())
                    .and_then(|name| arrays_by_name.get(name))
                {
                    roots.push(*array);
                }
            }
            _ => {}
        }
    }
    roots.sort_by_key(|array| array.start_byte());
    roots.dedup_by_key(|array| array.start_byte());

    let mut facts = Vec::new();
    for array in roots {
        collect_route_objects(language, file_path, content, array, None, &mut facts, 0);
    }
    facts
}

fn collect_kind<'t>(node: Node<'t>, kind: &str, found: &mut Vec<Node<'t>>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == kind {
        found.push(node);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_kind(child, kind, found, child_depth);
    }
}

/// `const routes: Routes = [...]` or `const routes: Route[] = [...]`.
fn is_routes_annotation(declarator: Node, content: &str, imports: &JsImportIndex) -> bool {
    let Some(annotation) = declarator
        .child_by_field_name("type")
        .and_then(|annotation| content.get(annotation.byte_range()))
    else {
        return false;
    };
    let annotation = annotation.trim_start_matches(':').trim();
    let (name, is_array) = match annotation.strip_suffix("[]") {
        Some(element) => (element.trim(), true),
        None => (annotation, false),
    };
    match imports.angular_router.get(name).map(String::as_str) {
        Some("Routes") => !is_array,
        Some("Route") => is_array,
        _ => false,
    }
}

/// `RouterModule.forRoot(routes)`, `RouterModule.forChild(routes)`, or
/// `provideRouter(routes)`.
fn is_router_registration(call: Node, content: &str, imports: &JsImportIndex) -> bool {
    let Some(function) = call.child_by_field_name("function") else {
        return false;
    };
    let text = |node: Node| content.get(node.byte_range()).unwrap_or_default();
    match function.kind() {
        "identifier" => {
            imports
                .angular_router
                .get(text(function))
                .map(String::as_str)
                == Some("provideRouter")
        }
        "member_expression" => {
            let object = function.child_by_field_name("object");
            let property = function.child_by_field_name("property");
            object.is_some_and(|object| {
                imports.angular_router.get(text(object)).map(String::as_str) == Some("RouterModule")
            }) && property.is_some_and(|property| matches!(text(property), "forRoot" | "forChild"))
        }
        _ => false,
    }
}

fn collect_route_objects(
    language: &str,
    file_path: &str,
    content: &str,
    array: Node,
    parent_route_path: Option<&str>,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = array.walk();
    for object in array.named_children(&mut cursor) {
        if object.kind() != "object" {
            continue;
        }
        let properties = object_properties(object, content);
        let Some(route_path) = properties
            .get("path")
            .and_then(|value| static_string_value(*value, content))
        else {
            continue;
        };
        let effective_route_template =
            join_frontend_route_paths(parent_route_path.unwrap_or_default(), &route_path);

        let mut metadata = base_metadata("frontend_navigation");
        insert_string(&mut metadata, "framework", "angular");
        insert_string(&mut metadata, "library", "angular_router");
        insert_string(&mut metadata, "source_kind", "route_object");
        insert_string(&mut metadata, "route_path", &route_path);
        insert_string(&mut metadata, "route_source", "string_literal");
        insert_string(
            &mut metadata,
            "effective_route_template",
            &effective_route_template,
        );
        if let Some(parent) = parent_route_path {
            insert_string(&mut metadata, "parent_route_path", parent);
        }
        if let Some(component) = properties
            .get("component")
            .filter(|value| value.kind() == "identifier")
            .and_then(|value| content.get(value.byte_range()))
        {
            insert_string(&mut metadata, "route_component", component);
        }
        if let Some(redirect) = properties
            .get("redirectTo")
            .and_then(|value| static_string_value(*value, content))
        {
            insert_string(&mut metadata, "redirect_to", &redirect);
        }
        for (property, key) in [
            ("loadChildren", "lazy_module_source"),
            ("loadComponent", "lazy_component_source"),
        ] {
            if let Some(source) = properties
                .get(property)
                .and_then(|value| dynamic_import_source(*value, content))
            {
                insert_string(&mut metadata, key, &source);
            }
        }
        facts.push(fact_for_node(
            file_path,
            language,
            ANGULAR_ROUTE_DEFINITION_PATTERN_ID,
            "route_definition",
            object,
            metadata,
        ));

        if let Some(children) = properties
            .get("children")
            .filter(|value| value.kind() == "array")
        {
            collect_route_objects(
                language,
                file_path,
                content,
                *children,
                Some(&effective_route_template),
                facts,
                child_depth,
            );
        }
    }
}

fn object_properties<'t>(object: Node<'t>, content: &str) -> HashMap<String, Node<'t>> {
    let mut properties = HashMap::new();
    let mut cursor = object.walk();
    for pair in object.named_children(&mut cursor) {
        if pair.kind() != "pair" {
            continue;
        }
        let (Some(key), Some(value)) = (
            pair.child_by_field_name("key"),
            pair.child_by_field_name("value"),
        ) else {
            continue;
        };
        let key = match key.kind() {
            "property_identifier" => content.get(key.byte_range()).map(str::to_string),
            _ => static_string_value(key, content),
        };
        if let Some(key) = key {
            properties.insert(key, value);
        }
    }
    properties
}

/// The source of the first `import('...')` inside a lazy-load callback.
fn dynamic_import_source(value: Node, content: &str) -> Option<String> {
    let mut calls = Vec::new();
    collect_calls(value, &mut calls, 0);
    calls.into_iter().find_map(|call| {
        call.child_by_field_name("function")
            .filter(|function| function.kind() == "import")?;
        let arguments = call.child_by_field_name("arguments")?;
        let mut cursor = arguments.walk();
        let source = arguments.named_children(&mut cursor).next()?;
        static_string_value(source, content)
    })
}

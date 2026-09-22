//! R framework facts: plumber routes and Shiny apps.
//!
//! plumber routes come from `#* @get /path` annotation blocks above a handler
//! and from `pr_get("/path", handler)` router calls. Shiny facts come from
//! input and output widgets, `output$id <- render*()` bindings, reactives and
//! observers, modules, and `shinyApp()`, in a file that mentions shiny.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_node, insert_string, insert_string_array};
use super::{
    PLUMBER_ROUTE_PATTERN_ID, SHINY_APP_PATTERN_ID, SHINY_INPUT_PATTERN_ID,
    SHINY_MODULE_PATTERN_ID, SHINY_OUTPUT_PATTERN_ID, SHINY_REACTIVE_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::types::StructuralFact;
use crate::r::plumber::{PLUMBER_VERBS, annotated_routes};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const SHINY_INPUT_WIDGETS: &[&str] = &["actionButton", "actionLink", "radioButtons"];

const SHINY_REACTIVES: &[&str] = &[
    "reactive",
    "eventReactive",
    "reactiveVal",
    "reactiveValues",
    "reactivePoll",
    "reactiveFileReader",
    "observe",
    "observeEvent",
];

struct Scan<'a> {
    language: &'a str,
    file_path: &'a str,
    content: &'a str,
    shiny: bool,
}

pub(super) fn collect_r_framework_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let scan = Scan {
        language,
        file_path,
        content,
        shiny: content.contains("shiny"),
    };
    let mut facts = Vec::new();
    visit(tree.root_node(), &scan, &mut facts, 0);
    facts
}

fn visit(node: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "function_definition" if node.parent().is_some_and(|p| p.kind() == "program") => {
            for (verb, path) in annotated_routes(scan.content, node) {
                push_route(node, &verb, &path, "annotation", scan, facts);
            }
        }
        "call" => {
            push_router_call(node, scan, facts);
            if scan.shiny {
                push_shiny_call(node, scan, facts);
            }
        }
        "binary_operator" if scan.shiny => push_shiny_render(node, scan, facts),
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, scan, facts, child_depth);
    }
}

fn push_router_call(call: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    let Some(name) = call_name(call, scan.content) else {
        return;
    };
    let Some(verb) = name
        .strip_prefix("pr_")
        .filter(|verb| PLUMBER_VERBS.contains(verb))
    else {
        return;
    };
    let args = unnamed_arguments(call);
    let Some(path) = args
        .first()
        .and_then(|path| static_string(*path, scan.content))
        .filter(|path| path.starts_with('/'))
    else {
        return;
    };
    push_route(
        call,
        &verb.to_ascii_uppercase(),
        &path,
        "router_call",
        scan,
        facts,
    );
}

fn push_route(
    node: Node,
    verb: &str,
    path: &str,
    api_style: &str,
    scan: &Scan<'_>,
    facts: &mut Vec<StructuralFact>,
) {
    let mut metadata = base_metadata("framework", "plumber");
    insert_string(&mut metadata, "api_style", api_style);
    insert_string(&mut metadata, "route_template", path);
    let normalized = normalize_route_template(&strip_param_types(path), ParamFlavor::AngleBrackets);
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
    insert_string(&mut metadata, "verb", verb);
    insert_string(&mut metadata, "verb_source", "attested");
    push(
        facts,
        scan,
        PLUMBER_ROUTE_PATTERN_ID,
        "route",
        node,
        metadata,
    );
}

/// plumber writes a typed parameter as `<name:type>`; drop the type so the
/// shared angle-bracket normalizer sees `<name>`.
fn strip_param_types(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut in_param = false;
    let mut skipping = false;
    for ch in path.chars() {
        match ch {
            '<' => {
                in_param = true;
                skipping = false;
                out.push(ch);
            }
            '>' => {
                in_param = false;
                skipping = false;
                out.push(ch);
            }
            ':' if in_param => skipping = true,
            _ if skipping => {}
            _ => out.push(ch),
        }
    }
    out
}

fn push_shiny_call(call: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    let Some(name) = call_name(call, scan.content) else {
        return;
    };
    let name = name.as_str();
    let is_input = (name.ends_with("Input") && !name.starts_with("update"))
        || SHINY_INPUT_WIDGETS.contains(&name);
    if is_input {
        if let Some(id) = widget_id(call, "inputId", scan.content) {
            let mut metadata = base_metadata("framework", "shiny");
            insert_string(&mut metadata, "input_id", &id);
            insert_string(&mut metadata, "widget", name);
            push(facts, scan, SHINY_INPUT_PATTERN_ID, "input", call, metadata);
        }
        return;
    }
    if name.ends_with("Output") {
        if let Some(id) = widget_id(call, "outputId", scan.content) {
            let mut metadata = base_metadata("framework", "shiny");
            insert_string(&mut metadata, "role", "placeholder");
            insert_string(&mut metadata, "output_id", &id);
            insert_string(&mut metadata, "function", name);
            push(
                facts,
                scan,
                SHINY_OUTPUT_PATTERN_ID,
                "output",
                call,
                metadata,
            );
        }
        return;
    }
    if SHINY_REACTIVES.contains(&name) {
        let mut metadata = base_metadata("framework", "shiny");
        insert_string(&mut metadata, "reactive_kind", name);
        if let Some(bound) = assigned_name(call, scan.content) {
            insert_string(&mut metadata, "name", &bound);
        }
        if matches!(name, "observeEvent" | "eventReactive")
            && let Some(trigger) = unnamed_arguments(call).first()
        {
            insert_string(&mut metadata, "trigger", text(scan.content, *trigger));
        }
        push(
            facts,
            scan,
            SHINY_REACTIVE_PATTERN_ID,
            "reactive",
            call,
            metadata,
        );
        return;
    }
    match name {
        "moduleServer" | "callModule" => {
            let args = unnamed_arguments(call);
            let (id_index, function_index) = if name == "moduleServer" {
                (0, 1)
            } else {
                (1, 0)
            };
            let mut metadata = base_metadata("framework", "shiny");
            insert_string(&mut metadata, "module_call", name);
            if let Some(id) = args
                .get(id_index)
                .and_then(|id| static_string(*id, scan.content))
            {
                insert_string(&mut metadata, "module_id", &id);
            }
            if let Some(function) = args
                .get(function_index)
                .filter(|function| function.kind() == "identifier")
            {
                insert_string(
                    &mut metadata,
                    "server_function",
                    text(scan.content, *function),
                );
            }
            push(
                facts,
                scan,
                SHINY_MODULE_PATTERN_ID,
                "module",
                call,
                metadata,
            );
        }
        "shinyApp" => {
            let mut metadata = base_metadata("framework", "shiny");
            for key in ["ui", "server"] {
                if let Some(value) = argument_value(call, key, scan.content)
                    .filter(|value| value.kind() == "identifier")
                {
                    insert_string(&mut metadata, key, text(scan.content, value));
                }
            }
            push(facts, scan, SHINY_APP_PATTERN_ID, "app", call, metadata);
        }
        _ => {}
    }
}

/// `output$id <- render*(...)`.
fn push_shiny_render(node: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    let (Some(lhs), Some(operator), Some(rhs)) = (
        node.child_by_field_name("lhs"),
        node.child_by_field_name("operator"),
        node.child_by_field_name("rhs"),
    ) else {
        return;
    };
    if !matches!(text(scan.content, operator), "<-" | "=") || lhs.kind() != "extract_operator" {
        return;
    }
    let (Some(object), Some(member)) = (
        lhs.child_by_field_name("lhs"),
        lhs.child_by_field_name("rhs"),
    ) else {
        return;
    };
    if text(scan.content, object) != "output" {
        return;
    }
    let Some(function) = call_name(rhs, scan.content).filter(|name| name.starts_with("render"))
    else {
        return;
    };
    let mut metadata = base_metadata("framework", "shiny");
    insert_string(&mut metadata, "role", "render");
    insert_string(
        &mut metadata,
        "output_id",
        text(scan.content, member).trim_matches('`'),
    );
    insert_string(&mut metadata, "function", &function);
    push(
        facts,
        scan,
        SHINY_OUTPUT_PATTERN_ID,
        "output",
        node,
        metadata,
    );
}

fn push(
    facts: &mut Vec<StructuralFact>,
    scan: &Scan<'_>,
    pattern_id: &str,
    capture_name: &str,
    node: Node,
    metadata: HashMap<String, Value>,
) {
    facts.push(fact_for_node(
        scan.file_path,
        scan.language,
        pattern_id,
        capture_name,
        node,
        metadata,
    ));
}

/// The called function name, bare or `pkg::name`.
fn call_name(call: Node, content: &str) -> Option<String> {
    if call.kind() != "call" {
        return None;
    }
    let function = call.child_by_field_name("function")?;
    let name = match function.kind() {
        "identifier" => function,
        "namespace_operator" => function.child_by_field_name("rhs")?,
        _ => return None,
    };
    Some(text(content, name).to_string())
}

fn unnamed_arguments(call: Node) -> Vec<Node> {
    let Some(args) = call.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = args.walk();
    args.children_by_field_name("argument", &mut cursor)
        .filter(|argument| argument.child_by_field_name("name").is_none())
        .filter_map(|argument| argument.child_by_field_name("value"))
        .collect()
}

fn argument_value<'t>(call: Node<'t>, name: &str, content: &str) -> Option<Node<'t>> {
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    args.children_by_field_name("argument", &mut cursor)
        .find(|argument| {
            argument
                .child_by_field_name("name")
                .is_some_and(|key| text(content, key) == name)
        })
        .and_then(|argument| argument.child_by_field_name("value"))
}

/// The widget id: the named id argument, else the first unnamed argument.
fn widget_id(call: Node, key: &str, content: &str) -> Option<String> {
    argument_value(call, key, content)
        .or_else(|| unnamed_arguments(call).first().copied())
        .and_then(|value| static_string(value, content))
}

/// The name a call's value is assigned to (`name <- call(...)`).
fn assigned_name(call: Node, content: &str) -> Option<String> {
    let parent = call.parent()?;
    if parent.kind() != "binary_operator" || parent.child_by_field_name("rhs") != Some(call) {
        return None;
    }
    let operator = text(content, parent.child_by_field_name("operator")?);
    if !matches!(operator, "<-" | "=" | "<<-") {
        return None;
    }
    let lhs = parent.child_by_field_name("lhs")?;
    (lhs.kind() == "identifier").then(|| text(content, lhs).to_string())
}

fn static_string(node: Node, content: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let mut cursor = node.walk();
    let value = node
        .children(&mut cursor)
        .find(|child| child.kind() == "string_content")
        .map(|inner| text(content, inner))
        .unwrap_or_default();
    (!value.contains('\\')).then(|| value.to_string())
}

fn text<'a>(content: &'a str, node: Node<'_>) -> &'a str {
    content.get(node.byte_range()).unwrap_or_default()
}

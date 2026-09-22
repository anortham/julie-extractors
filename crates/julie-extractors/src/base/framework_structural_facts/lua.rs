//! Lua framework facts: Lapis routes, Neovim user commands, autocommands and
//! keymaps, LÖVE callbacks, and lazy.nvim plugin specs.
//!
//! Every fact needs static string arguments; a computed path, name, or event
//! emits nothing.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_node, insert_string, insert_string_array};
use super::{
    LAPIS_ROUTE_PATTERN_ID, LAZY_NVIM_PLUGIN_SPEC_PATTERN_ID, LOVE_CALLBACK_PATTERN_ID,
    NEOVIM_AUTOCMD_PATTERN_ID, NEOVIM_KEYMAP_PATTERN_ID, NEOVIM_USER_COMMAND_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const LAPIS_VERBS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

const LOVE_CALLBACKS: &[&str] = &[
    "conf",
    "directorydropped",
    "displayrotated",
    "draw",
    "errorhandler",
    "filedropped",
    "focus",
    "gamepadaxis",
    "gamepadpressed",
    "gamepadreleased",
    "joystickadded",
    "joystickaxis",
    "joystickhat",
    "joystickpressed",
    "joystickreleased",
    "joystickremoved",
    "keypressed",
    "keyreleased",
    "load",
    "lowmemory",
    "mousefocus",
    "mousemoved",
    "mousepressed",
    "mousereleased",
    "quit",
    "resize",
    "run",
    "textedited",
    "textinput",
    "threaderror",
    "touchmoved",
    "touchpressed",
    "touchreleased",
    "update",
    "visible",
    "wheelmoved",
];

struct Scan<'a> {
    language: &'a str,
    file_path: &'a str,
    content: &'a str,
    lapis: bool,
}

pub(super) fn collect_lua_framework_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let scan = Scan {
        language,
        file_path,
        content,
        lapis: content.contains("lapis"),
    };
    let mut facts = Vec::new();
    visit(tree.root_node(), &scan, &mut facts, 0);
    collect_lazy_specs(tree.root_node(), &scan, &mut facts);
    facts
}

fn visit(node: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "function_call" => push_call_fact(node, scan, facts),
        "function_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                push_love_callback(node, name, scan, facts);
            }
        }
        "assignment_statement" => push_love_assignment(node, scan, facts),
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

fn push_call_fact(call: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    let (Some(callee), Some(arguments)) = (
        call.child_by_field_name("name"),
        call.child_by_field_name("arguments"),
    ) else {
        return;
    };
    let args = argument_nodes(arguments);
    if callee.kind() == "method_index_expression" {
        if scan.lapis {
            push_lapis_route(call, callee, &args, scan, facts);
        }
        return;
    }
    let callee_text = text(scan.content, callee);
    match callee_text {
        "vim.api.nvim_create_user_command" => {
            push_user_command(call, &args, 0, "global", scan, facts)
        }
        "vim.api.nvim_buf_create_user_command" => {
            push_user_command(call, &args, 1, "buffer", scan, facts)
        }
        "vim.api.nvim_create_autocmd" => push_autocmd(call, &args, scan, facts),
        "vim.keymap.set" | "vim.api.nvim_set_keymap" => push_keymap(call, &args, 0, scan, facts),
        "vim.api.nvim_buf_set_keymap" => push_keymap(call, &args, 1, scan, facts),
        _ => {}
    }
}

fn push_lapis_route(
    call: Node,
    callee: Node,
    args: &[Node],
    scan: &Scan<'_>,
    facts: &mut Vec<StructuralFact>,
) {
    let Some(method) = callee.child_by_field_name("method") else {
        return;
    };
    let method = text(scan.content, method);
    let is_verb = LAPIS_VERBS.contains(&method);
    if !is_verb && method != "match" {
        return;
    }
    let strings: Vec<String> = args
        .iter()
        .map_while(|arg| static_string(*arg, scan.content))
        .collect();
    let has_handler = args.len() > strings.len();
    let (route_name, route_template) = match strings.as_slice() {
        [path] => (None, path),
        [name, path] => (Some(name), path),
        _ => return,
    };
    if !has_handler || !route_template.starts_with('/') {
        return;
    }
    let mut metadata = base_metadata("framework", "lapis");
    insert_string(&mut metadata, "api_style", "dsl_routing");
    insert_string(&mut metadata, "route_template", route_template);
    let normalized = normalize_route_template(route_template, ParamFlavor::Colon);
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
    if let Some(route_name) = route_name {
        insert_string(&mut metadata, "route_name", route_name);
    }
    if is_verb {
        insert_string(&mut metadata, "verb", &method.to_ascii_uppercase());
        insert_string(&mut metadata, "verb_source", "attested");
    }
    push(facts, scan, LAPIS_ROUTE_PATTERN_ID, "route", call, metadata);
}

fn push_user_command(
    call: Node,
    args: &[Node],
    name_index: usize,
    scope: &str,
    scan: &Scan<'_>,
    facts: &mut Vec<StructuralFact>,
) {
    let Some(name) = args
        .get(name_index)
        .and_then(|arg| static_string(*arg, scan.content))
    else {
        return;
    };
    let mut metadata = base_metadata("framework", "neovim");
    insert_string(&mut metadata, "command_name", &name);
    insert_string(&mut metadata, "scope", scope);
    if let Some(desc) = args
        .get(name_index + 2)
        .and_then(|options| table_string_field(*options, "desc", scan.content))
    {
        insert_string(&mut metadata, "desc", &desc);
    }
    push(
        facts,
        scan,
        NEOVIM_USER_COMMAND_PATTERN_ID,
        "user_command",
        call,
        metadata,
    );
}

fn push_autocmd(call: Node, args: &[Node], scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    let Some(events) = args
        .first()
        .and_then(|events| string_or_string_list(*events, scan.content))
    else {
        return;
    };
    let mut metadata = base_metadata("framework", "neovim");
    insert_string_array(&mut metadata, "events", events);
    if let Some(options) = args.get(1) {
        if let Some(patterns) = table_field_value(*options, "pattern", scan.content)
            .and_then(|pattern| string_or_string_list(pattern, scan.content))
        {
            insert_string_array(&mut metadata, "patterns", patterns);
        }
        if let Some(group) = table_string_field(*options, "group", scan.content) {
            insert_string(&mut metadata, "group", &group);
        }
        if let Some(desc) = table_string_field(*options, "desc", scan.content) {
            insert_string(&mut metadata, "desc", &desc);
        }
    }
    push(
        facts,
        scan,
        NEOVIM_AUTOCMD_PATTERN_ID,
        "autocmd",
        call,
        metadata,
    );
}

fn push_keymap(
    call: Node,
    args: &[Node],
    mode_index: usize,
    scan: &Scan<'_>,
    facts: &mut Vec<StructuralFact>,
) {
    let (Some(modes), Some(lhs)) = (
        args.get(mode_index)
            .and_then(|modes| string_or_string_list(*modes, scan.content)),
        args.get(mode_index + 1)
            .and_then(|lhs| static_string(*lhs, scan.content)),
    ) else {
        return;
    };
    let mut metadata = base_metadata("framework", "neovim");
    insert_string_array(&mut metadata, "modes", modes);
    insert_string(&mut metadata, "lhs", &lhs);
    if let Some(desc) = args
        .get(mode_index + 3)
        .and_then(|options| table_string_field(*options, "desc", scan.content))
    {
        insert_string(&mut metadata, "desc", &desc);
    }
    push(
        facts,
        scan,
        NEOVIM_KEYMAP_PATTERN_ID,
        "keymap",
        call,
        metadata,
    );
}

fn push_love_callback(node: Node, name: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    if name.kind() != "dot_index_expression" {
        return;
    }
    let (Some(table), Some(field)) = (
        name.child_by_field_name("table"),
        name.child_by_field_name("field"),
    ) else {
        return;
    };
    let callback = text(scan.content, field);
    if text(scan.content, table) != "love" || !LOVE_CALLBACKS.contains(&callback) {
        return;
    }
    let mut metadata = base_metadata("framework", "love");
    insert_string(&mut metadata, "callback", callback);
    push(
        facts,
        scan,
        LOVE_CALLBACK_PATTERN_ID,
        "callback",
        node,
        metadata,
    );
}

fn push_love_assignment(node: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    let Some(target) = child_of_kind(node, "variable_list").and_then(|list| list.named_child(0))
    else {
        return;
    };
    let is_function = child_of_kind(node, "expression_list")
        .and_then(|list| list.named_child(0))
        .is_some_and(|value| value.kind() == "function_definition");
    if is_function {
        push_love_callback(node, target, scan, facts);
    }
}

fn collect_lazy_specs(root: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    let mut cursor = root.walk();
    let Some(returned) = root
        .children(&mut cursor)
        .filter(|statement| statement.kind() == "return_statement")
        .find_map(|statement| child_of_kind(statement, "expression_list"))
        .and_then(|list| list.named_child(0))
        .filter(|value| value.kind() == "table_constructor")
    else {
        return;
    };
    if lazy_plugin_id(returned, scan.content).is_some() {
        push_lazy_spec(returned, scan, facts);
        return;
    }
    for value in positional_values(returned) {
        if value.kind() == "table_constructor" && lazy_plugin_id(value, scan.content).is_some() {
            push_lazy_spec(value, scan, facts);
        }
    }
}

fn push_lazy_spec(spec: Node, scan: &Scan<'_>, facts: &mut Vec<StructuralFact>) {
    let Some(plugin) = lazy_plugin_id(spec, scan.content) else {
        return;
    };
    let mut metadata = base_metadata("framework", "lazy.nvim");
    insert_string(&mut metadata, "plugin", &plugin);
    if let Some(dependencies) = table_field_value(spec, "dependencies", scan.content) {
        let names: Vec<String> = if let Some(name) = static_string(dependencies, scan.content) {
            vec![name]
        } else {
            positional_values(dependencies)
                .into_iter()
                .filter_map(|dependency| {
                    static_string(dependency, scan.content)
                        .or_else(|| lazy_plugin_id(dependency, scan.content))
                })
                .collect()
        };
        if !names.is_empty() {
            insert_string_array(&mut metadata, "dependencies", names);
        }
    }
    for (field, key) in [
        ("cmd", "commands"),
        ("event", "events"),
        ("ft", "filetypes"),
    ] {
        if let Some(values) = table_field_value(spec, field, scan.content)
            .and_then(|value| string_or_string_list(value, scan.content))
        {
            insert_string_array(&mut metadata, key, values);
        }
    }
    push(
        facts,
        scan,
        LAZY_NVIM_PLUGIN_SPEC_PATTERN_ID,
        "plugin_spec",
        spec,
        metadata,
    );
}

/// The `owner/repo` id in a lazy.nvim spec's first positional field.
fn lazy_plugin_id(spec: Node, content: &str) -> Option<String> {
    if spec.kind() != "table_constructor" {
        return None;
    }
    let id = static_string(*positional_values(spec).first()?, content)?;
    let (owner, repo) = id.split_once('/')?;
    let is_part = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (is_part(owner) && is_part(repo)).then_some(id)
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

fn argument_nodes(arguments: Node) -> Vec<Node> {
    if arguments.kind() != "arguments" {
        return vec![arguments];
    }
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .collect()
}

fn positional_values(table: Node) -> Vec<Node> {
    let mut cursor = table.walk();
    table
        .named_children(&mut cursor)
        .filter(|field| field.kind() == "field" && field.child_by_field_name("name").is_none())
        .filter_map(|field| field.child_by_field_name("value"))
        .collect()
}

fn table_field_value<'t>(table: Node<'t>, key: &str, content: &str) -> Option<Node<'t>> {
    if table.kind() != "table_constructor" {
        return None;
    }
    let mut cursor = table.walk();
    table
        .named_children(&mut cursor)
        .filter(|field| field.kind() == "field")
        .find(|field| {
            field
                .child_by_field_name("name")
                .is_some_and(|name| name.kind() == "identifier" && text(content, name) == key)
        })
        .and_then(|field| field.child_by_field_name("value"))
}

fn table_string_field(table: Node, key: &str, content: &str) -> Option<String> {
    table_field_value(table, key, content).and_then(|value| static_string(value, content))
}

/// A static string, or a table of static strings.
fn string_or_string_list(node: Node, content: &str) -> Option<Vec<String>> {
    if let Some(value) = static_string(node, content) {
        return Some(vec![value]);
    }
    if node.kind() != "table_constructor" {
        return None;
    }
    let values: Option<Vec<String>> = positional_values(node)
        .into_iter()
        .map(|value| static_string(value, content))
        .collect();
    values.filter(|values| !values.is_empty())
}

/// The contents of a string literal with no escape sequences.
fn static_string(node: Node, content: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let value = child_of_kind(node, "string_content")
        .map(|inner| text(content, inner))
        .unwrap_or_default();
    (!value.contains('\\')).then(|| value.to_string())
}

fn child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn text<'a>(content: &'a str, node: Node<'_>) -> &'a str {
    content.get(node.byte_range()).unwrap_or_default()
}

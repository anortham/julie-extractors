//! Giraffe (F#) combinator routes: `route "/x"`, `routef "/x/%i" h`,
//! `subRoute "/api" (...)`, with the verb from an enclosing `GET >=> ...`.

use tree_sitter::{Node, Tree};

use super::GIRAFFE_ROUTE_PATTERN_ID;
use super::helpers::{base_metadata, fact_for_node, insert_string, node_text};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const ROUTE_FUNCTIONS: &[&str] = &[
    "route",
    "routeCi",
    "routef",
    "routeCif",
    "routeStartsWith",
    "routeStartsWithCi",
    "subRoute",
    "subRouteCi",
];

const VERBS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

pub(super) fn collect_giraffe_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    visit(
        tree.root_node(),
        language,
        file_path,
        content,
        &mut facts,
        0,
    );
    facts
}

fn visit(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "application_expression"
        && let Some((function, template, head_verb)) = route_call(node, content)
    {
        let mut metadata = base_metadata("framework", "giraffe");
        insert_string(&mut metadata, "api_style", "combinator_routing");
        insert_string(&mut metadata, "route_function", function);
        insert_string(&mut metadata, "route_template", &template);
        let prefix = sub_route_prefix(node, content);
        let effective = match &prefix {
            Some(prefix) => {
                let effective = join_route_templates(prefix, &template);
                insert_string(&mut metadata, "route_group_prefix", prefix);
                insert_string(&mut metadata, "effective_route_template", &effective);
                effective
            }
            None => template.clone(),
        };
        let normalized =
            normalize_route_template(&format_to_braces(&effective), ParamFlavor::Braces);
        insert_string(
            &mut metadata,
            "normalized_route_template",
            &normalized.template,
        );
        if let Some(verb) = head_verb.or_else(|| enclosing_verb(node, content)) {
            insert_string(&mut metadata, "verb", verb);
        }
        facts.push(fact_for_node(
            file_path,
            language,
            GIRAFFE_ROUTE_PATTERN_ID,
            "route",
            node,
            metadata,
        ));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, language, file_path, content, facts, child_depth);
    }
}

/// `route "/x"`: the route function, its string-literal template, and the
/// verb when the grammar folds `POST >=> route` into the call head.
fn route_call(
    node: Node<'_>,
    content: &str,
) -> Option<(&'static str, String, Option<&'static str>)> {
    let mut cursor = node.walk();
    let mut children = node.named_children(&mut cursor);
    let head = children.next()?;
    let (head, head_verb) = match head.kind() {
        "long_identifier_or_op" => (head, None),
        "infix_expression" => {
            let (left, right) = compose_operands(head, content)?;
            (right, chain_verb(left, content))
        }
        _ => return None,
    };
    if head.kind() != "long_identifier_or_op" {
        return None;
    }
    let head_text = node_text(content, head)?.trim();
    let function = ROUTE_FUNCTIONS.iter().find(|name| **name == head_text)?;
    let argument = children
        .next()
        .filter(|argument| argument.kind() == "const")?;
    let mut argument_cursor = argument.walk();
    let string = argument
        .named_children(&mut argument_cursor)
        .find(|child| child.kind() == "string")?;
    let text = node_text(content, string)?;
    Some((
        function,
        text.strip_prefix('"')?.strip_suffix('"')?.to_string(),
        head_verb,
    ))
}

/// The operands of `left >=> right`.
fn compose_operands<'t>(infix: Node<'t>, content: &str) -> Option<(Node<'t>, Node<'t>)> {
    let mut cursor = infix.walk();
    let children: Vec<Node> = infix.named_children(&mut cursor).collect();
    match children.as_slice() {
        [left, op, right] if node_text(content, *op).map(str::trim) == Some(">=>") => {
            Some((*left, *right))
        }
        _ => None,
    }
}

/// Composed prefixes of the `subRoute "/p" (...)` calls whose handler
/// argument contains `node`.
fn sub_route_prefix(node: Node<'_>, content: &str) -> Option<String> {
    let mut prefixes = Vec::new();
    let mut current = node.parent();
    while let Some(candidate) = current {
        if candidate.kind() == "application_expression"
            && let Some(inner) = first_named(candidate)
            && inner.kind() == "application_expression"
            && !contains(inner, node)
            && let Some((function, template, _)) = route_call(inner, content)
            && function.starts_with("subRoute")
        {
            prefixes.push(template);
        }
        current = candidate.parent();
    }
    prefixes
        .into_iter()
        .rev()
        .reduce(|outer, inner| join_route_templates(&outer, &inner))
}

/// The verb of the nearest `VERB >=> handler` chain whose handler side holds
/// `node`.
fn enclosing_verb(node: Node<'_>, content: &str) -> Option<&'static str> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "infix_expression" => {
                if let Some((left, right)) = compose_operands(candidate, content)
                    && contains(right, node)
                    && let Some(verb) = chain_verb(left, content)
                {
                    return Some(verb);
                }
            }
            // `GET >=> choose [...]` parses as `(GET >=> choose) [...]`.
            "application_expression" => {
                if let Some(head) = first_named(candidate)
                    && head.kind() == "infix_expression"
                    && !contains(head, node)
                    && let Some((left, _)) = compose_operands(head, content)
                    && let Some(verb) = chain_verb(left, content)
                {
                    return Some(verb);
                }
            }
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

/// `GET` or the last element of a `>=>` chain ending in a verb.
fn chain_verb(node: Node<'_>, content: &str) -> Option<&'static str> {
    let text = node_text(content, node)?.trim();
    let last = text.rsplit(">=>").next().unwrap_or(text).trim();
    VERBS.iter().find(|verb| **verb == last).copied()
}

/// Giraffe `%i`/`%s`/`%d` format placeholders become `{argN}` segments.
fn format_to_braces(template: &str) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    let mut index = 0;
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => out.push('%'),
                Some(_) => {
                    index += 1;
                    out.push_str(&format!("{{arg{index}}}"));
                }
                None => out.push('%'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn first_named(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

fn contains(outer: Node<'_>, inner: Node<'_>) -> bool {
    outer.start_byte() <= inner.start_byte() && outer.end_byte() >= inner.end_byte()
}

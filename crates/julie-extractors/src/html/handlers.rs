//! Script held in attribute values: inline event handlers (`onclick`),
//! Alpine (`@click`, `x-on:*`, `x-data`, `x-init`, `x-effect`), htmx
//! (`hx-on*`), and Stimulus `data-action` descriptors.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use crate::base::relationship_resolution::StructuredPendingRelationship;
use crate::base::{
    BaseExtractor, Identifier, IdentifierKind, Literal, NormalizedSpan, Relationship, Symbol,
    SymbolKind,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

use super::helpers::HTMLHelpers;

/// Rows produced by attribute-held script, in host coordinates.
#[derive(Default)]
pub(super) struct HandlerRows {
    pub(super) identifiers: Vec<Identifier>,
    pub(super) literals: Vec<Literal>,
    pub(super) relationships: Vec<Relationship>,
    pub(super) pending: Vec<StructuredPendingRelationship>,
}

pub(super) fn collect_handler_rows(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> HandlerRows {
    let functions = crate::embedded::unique_by_name(
        symbols
            .iter()
            .filter(|symbol| is_global_function(base, tree, symbol)),
    );
    let mut rows = HandlerRows::default();
    let mut attributes = Vec::new();
    collect_attributes(tree.root_node(), &mut attributes, 0);
    for attribute in attributes {
        let Some((name, value)) = attribute_name_and_value(attribute) else {
            continue;
        };
        let name = base.get_node_text(&name).to_ascii_lowercase();
        let caller = innermost_symbol(base, attribute, symbols);
        let text = base.get_node_text(&value);
        if name == "data-action" {
            if let Some(caller) = caller {
                stimulus_identifiers(base, value, caller, &mut rows);
            }
        } else if is_script_attribute(&name) {
            script_rows(
                base,
                value.start_byte(),
                &text,
                caller,
                &functions,
                &mut rows,
                true,
            );
        } else if is_binding_attribute(&name) {
            for (offset, expression) in binding_expressions(&name, &text) {
                let start = value.start_byte() + offset;
                script_rows(
                    base, start, expression, caller, &functions, &mut rows, false,
                );
            }
        }
    }
    let mut texts = Vec::new();
    collect_text_nodes(tree.root_node(), &mut texts, 0);
    for text_node in texts {
        let caller = innermost_symbol(base, text_node, symbols);
        let text = base.get_node_text(&text_node);
        for (offset, expression) in interpolations(&text) {
            let start = text_node.start_byte() + offset;
            script_rows(
                base, start, expression, caller, &functions, &mut rows, false,
            );
        }
    }
    rows
}

/// Angular property, two-way, and structural bindings: `[value]`,
/// `[(ngModel)]`, `bind-value`, `*ngIf`.
fn is_binding_attribute(name: &str) -> bool {
    (name.starts_with('[') && name.ends_with(']'))
        || name.starts_with("bind-")
        || name.starts_with("bindon-")
        || (name.starts_with('*') && name.len() > 1)
}

/// The expressions of a binding value with their byte offsets. A structural
/// directive (`*ngFor="let user of users; trackBy: trackById"`) holds several
/// clauses; its `let` and `as` clauses declare template names.
fn binding_expressions<'t>(name: &str, value: &'t str) -> Vec<(usize, &'t str)> {
    if !name.starts_with('*') {
        return vec![(0, strip_pipes(value))];
    }
    let mut expressions = Vec::new();
    let mut offset = 0;
    for clause in value.split(';') {
        let clause_offset = offset;
        offset += clause.len() + 1;
        let trimmed = clause.trim_start();
        let lead = clause.len() - trimmed.len();
        let expression = if let Some(rest) = trimmed.strip_prefix("let ") {
            rest.find(" of ").map(|at| (4 + at + 4, &rest[at + 4..]))
        } else if let Some((key, rest)) = trimmed.split_once(':') {
            (!key.trim().contains(' ')).then(|| (key.len() + 1, rest))
        } else if expressions.is_empty() && !trimmed.contains(" as ") {
            Some((0, trimmed))
        } else {
            None
        };
        if let Some((relative, expression)) = expression {
            let expression = strip_pipes(expression);
            if !expression.trim().is_empty() {
                expressions.push((clause_offset + lead + relative, expression));
            }
        }
    }
    expressions
}

/// `{{ expression }}` interpolations in text, with their byte offsets.
fn interpolations(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("{{").map(|at| from + at + 2) {
        let Some(close) = text[open..].find("}}").map(|at| open + at) else {
            break;
        };
        found.push((open, strip_pipes(&text[open..close])));
        from = close + 2;
    }
    found
}

/// The expression before the first pipe (`value | uppercase`), as Angular
/// pipes and Jinja filters apply outside the JavaScript grammar.
fn strip_pipes(expression: &str) -> &str {
    let bytes = expression.as_bytes();
    let mut quote = None;
    let mut depth = 0usize;
    for (index, &byte) in bytes.iter().enumerate() {
        if let Some(open) = quote {
            if byte == open {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' | b'`' => quote = Some(byte),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'|' if depth == 0
                && bytes.get(index + 1) != Some(&b'|')
                && (index == 0 || bytes[index - 1] != b'|') =>
            {
                return &expression[..index];
            }
            _ => {}
        }
    }
    expression
}

fn collect_text_nodes<'t>(node: Node<'t>, texts: &mut Vec<Node<'t>>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "text" => {
            texts.push(node);
            return;
        }
        "script_element" | "style_element" => return,
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_text_nodes(child, texts, child_depth);
    }
}

/// Whether an inline handler can call the function: it is declared at the top
/// level of a classic script. Nested functions stay local to their parent, and
/// `<script type="module">` declarations stay module-scoped.
fn is_global_function(base: &BaseExtractor, tree: &Tree, symbol: &Symbol) -> bool {
    if symbol.kind != SymbolKind::Function || symbol.parent_id.is_some() {
        return false;
    }
    let start = symbol.start_byte as usize;
    let mut node = tree.root_node().descendant_for_byte_range(start, start);
    while let Some(current) = node {
        if current.kind() == "script_element" {
            return HTMLHelpers::extract_attributes(base, current)
                .get("type")
                .is_none_or(|script_type| script_type != "module");
        }
        node = current.parent();
    }
    true
}

/// The innermost symbol whose bytes hold the node: the element whose start
/// tag holds an attribute, or the element, block, or macro around a text node.
fn innermost_symbol<'s>(
    base: &BaseExtractor,
    node: Node<'_>,
    symbols: &'s [Symbol],
) -> Option<&'s Symbol> {
    let (start, end) = (node.start_byte() as u32, node.end_byte() as u32);
    symbols
        .iter()
        .filter(|symbol| symbol.file_path == base.file_path)
        .filter(|symbol| symbol.start_byte <= start && end <= symbol.end_byte)
        .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
}

fn collect_attributes<'t>(node: Node<'t>, attributes: &mut Vec<Node<'t>>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "attribute" {
        attributes.push(node);
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_attributes(child, attributes, child_depth);
    }
}

fn attribute_name_and_value(attribute: Node<'_>) -> Option<(Node<'_>, Node<'_>)> {
    let mut cursor = attribute.walk();
    let children: Vec<Node<'_>> = attribute.named_children(&mut cursor).collect();
    let name = children
        .iter()
        .find(|child| child.kind() == "attribute_name")?;
    let value = children.iter().find_map(|child| match child.kind() {
        "attribute_value" => Some(*child),
        "quoted_attribute_value" => {
            let mut cursor = child.walk();
            child
                .named_children(&mut cursor)
                .find(|inner| inner.kind() == "attribute_value")
        }
        _ => None,
    })?;
    Some((*name, value))
}

fn is_script_attribute(name: &str) -> bool {
    (name.len() > 2 && name.starts_with("on") && !name.contains(':'))
        || (name.len() > 2 && name.starts_with('(') && name.ends_with(')'))
        || name.starts_with('@')
        || name.starts_with("x-on:")
        || matches!(name, "x-data" | "x-init" | "x-effect")
        || name.starts_with("hx-on")
}

fn script_rows(
    base: &BaseExtractor,
    start: usize,
    text: &str,
    caller: Option<&Symbol>,
    functions: &HashMap<&str, Option<&Symbol>>,
    rows: &mut HandlerRows,
    handler: bool,
) {
    let Some((mut identifiers, literals)) =
        crate::embedded::extract_expression(&base.content, start, text, &base.file_path, handler)
    else {
        return;
    };
    let caller_id = caller.map(|caller| caller.id.clone());
    for identifier in &mut identifiers {
        identifier.containing_symbol_id = caller_id.clone();
    }
    if let Some(caller) = caller {
        crate::embedded::link_expression_identifiers(
            &base.content,
            caller,
            &identifiers,
            functions,
            None,
            &mut rows.relationships,
            &mut rows.pending,
        );
    }
    rows.identifiers.extend(identifiers);
    rows.literals
        .extend(literals.into_iter().map(|mut literal| {
            literal.containing_symbol_id = caller_id.clone();
            literal
        }));
}

/// `click->hello#greet:prevent` becomes a call identifier `greet` with
/// `controller: hello`.
fn stimulus_identifiers(
    base: &BaseExtractor,
    value: Node<'_>,
    caller: &Symbol,
    rows: &mut HandlerRows,
) {
    let text = base.get_node_text(&value);
    let mut search_from = 0;
    for descriptor in text.split_whitespace() {
        let Some(relative) = text[search_from..].find(descriptor) else {
            continue;
        };
        let descriptor_start = search_from + relative;
        search_from = descriptor_start + descriptor.len();

        let target = descriptor
            .rsplit_once("->")
            .map_or(descriptor, |(_, target)| target);
        let target_start = descriptor_start + descriptor.len() - target.len();
        let (controller, method) = target.split_once('#').unwrap_or(("", target));
        let method = method.split(':').next().unwrap_or(method);
        if !crate::embedded::is_member_path(method) || method.contains('.') {
            continue;
        }
        let method_offset = if controller.is_empty() && !target.contains('#') {
            0
        } else {
            controller.len() + 1
        };
        let start = value.start_byte() + target_start + method_offset;
        let Some(span) =
            NormalizedSpan::from_content_range(&base.content, start, start + method.len())
        else {
            continue;
        };
        let mut metadata = HashMap::new();
        if !controller.is_empty() {
            metadata.insert(
                "controller".to_string(),
                Value::String(controller.to_string()),
            );
        }
        let mut identifier = Identifier {
            id: String::new(),
            name: method.to_string(),
            kind: IdentifierKind::Call,
            language: base.language.clone(),
            file_path: base.file_path.clone(),
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            containing_symbol_id: Some(caller.id.clone()),
            target_symbol_id: None,
            confidence: 1.0,
            receiver_type: None,
            code_context: None,
            metadata: (!metadata.is_empty()).then_some(metadata),
        };
        identifier.refresh_id();
        rows.identifiers.push(identifier);
    }
}

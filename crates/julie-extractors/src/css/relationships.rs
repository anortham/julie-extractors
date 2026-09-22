use crate::base::{BaseExtractor, NormalizedSpan, Relationship, RelationshipKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

struct Targets<'a> {
    custom_properties: HashMap<String, Vec<&'a Symbol>>,
    keyframes: HashMap<String, Vec<&'a Symbol>>,
}

/// One `references` edge per `var(--x)` or animation name node, from the
/// innermost symbol whose byte range holds the node to every same-name
/// declaration in the file.
pub(super) fn extract_relationships(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let targets = Targets {
        custom_properties: symbols_by_metadata(symbols, "property"),
        keyframes: symbols_by_metadata(symbols, "animationName"),
    };
    let mut relationships = Vec::new();
    let mut seen = HashSet::new();
    let mut references = Vec::new();
    collect_references(base, tree.root_node(), &targets, &mut references, 0);
    for (node, candidates, reference_type) in references {
        for target in candidates {
            push_relationship(
                base,
                symbols,
                target,
                node,
                reference_type,
                &mut seen,
                &mut relationships,
            );
        }
    }
    relationships
}

fn collect_references<'t, 's>(
    base: &BaseExtractor,
    node: Node<'t>,
    targets: &'s Targets<'s>,
    references: &mut Vec<(Node<'t>, &'s [&'s Symbol], &'static str)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "call_expression" => {
            if let Some(argument) = var_argument(base, node)
                && let Some(candidates) = targets
                    .custom_properties
                    .get(&base.get_node_text(&argument))
            {
                references.push((argument, candidates, "custom-property"));
            }
        }
        "declaration" if is_animation_declaration(base, node) => {
            let mut cursor = node.walk();
            for value in node.named_children(&mut cursor) {
                if value.kind() == "plain_value"
                    && let Some(candidates) = targets.keyframes.get(&base.get_node_text(&value))
                {
                    references.push((value, candidates, "keyframes"));
                }
            }
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_references(base, child, targets, references, child_depth);
    }
}

fn var_argument<'t>(base: &BaseExtractor, call: Node<'t>) -> Option<Node<'t>> {
    let mut cursor = call.walk();
    let children: Vec<Node<'t>> = call.named_children(&mut cursor).collect();
    let function_name = children
        .iter()
        .find(|child| child.kind() == "function_name")?;
    if base.get_node_text(function_name) != "var" {
        return None;
    }
    let arguments = children.iter().find(|child| child.kind() == "arguments")?;
    let mut cursor = arguments.walk();
    let first = arguments.named_children(&mut cursor).next()?;
    (first.kind() == "plain_value" && base.get_node_text(&first).starts_with("--")).then_some(first)
}

fn is_animation_declaration(base: &BaseExtractor, declaration: Node<'_>) -> bool {
    let mut cursor = declaration.walk();
    declaration
        .named_children(&mut cursor)
        .find(|child| child.kind() == "property_name")
        .is_some_and(|name| {
            let name = base.get_node_text(&name).to_ascii_lowercase();
            name == "animation" || name == "animation-name"
        })
}

fn innermost_symbol_containing<'a>(symbols: &'a [Symbol], node: Node<'_>) -> Option<&'a Symbol> {
    let (start, end) = (node.start_byte() as u32, node.end_byte() as u32);
    symbols
        .iter()
        .filter(|symbol| symbol.start_byte <= start && end <= symbol.end_byte)
        .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
}

fn symbols_by_metadata<'a>(symbols: &'a [Symbol], key: &str) -> HashMap<String, Vec<&'a Symbol>> {
    let mut by_name: HashMap<String, Vec<&'a Symbol>> = HashMap::new();
    for symbol in symbols {
        if let Some(value) = symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(key))
            .and_then(Value::as_str)
        {
            by_name.entry(value.to_string()).or_default().push(symbol);
        }
    }
    by_name
}

fn push_relationship(
    base: &BaseExtractor,
    symbols: &[Symbol],
    target: &Symbol,
    node: Node<'_>,
    reference_type: &str,
    seen: &mut HashSet<(String, String, u32, String)>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(source) =
        innermost_symbol_containing(symbols, node).filter(|source| source.id != target.id)
    else {
        return;
    };
    let reference_name = base.get_node_text(&node);
    let span = NormalizedSpan::from_node(&node);
    let line_number = span.start_line;
    let key = (
        source.id.clone(),
        target.id.clone(),
        line_number,
        reference_name.clone(),
    );
    if !seen.insert(key) {
        return;
    }

    let mut metadata = HashMap::new();
    metadata.insert(
        "referenceName".to_string(),
        Value::String(reference_name.clone()),
    );
    metadata.insert(
        "referenceType".to_string(),
        Value::String(reference_type.to_string()),
    );

    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}_{}",
            source.id,
            target.id,
            RelationshipKind::References,
            line_number,
            reference_name
        ),
        from_symbol_id: source.id.clone(),
        to_symbol_id: target.id.clone(),
        kind: RelationshipKind::References,
        file_path: base.file_path.clone(),
        line_number,
        span: Some(span),
        reference_site_is_exact: true,
        confidence: 1.0,
        metadata: Some(metadata),
    });
}

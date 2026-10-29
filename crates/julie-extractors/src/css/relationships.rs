use super::custom_functions::dashed_functions;
use super::identifiers::composed_class_nodes;
use crate::base::{
    BaseExtractor, NormalizedSpan, Relationship, RelationshipKind, Symbol, SymbolKind,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

struct Targets<'a> {
    custom_properties: HashMap<String, Vec<&'a Symbol>>,
    keyframes: HashMap<String, Vec<&'a Symbol>>,
    class_rules: HashMap<String, Vec<&'a Symbol>>,
}

/// One `references` edge per `var(--x)` or animation name node, and one
/// `calls` edge per custom function call, from the innermost symbol whose byte
/// range holds the site to every same-name declaration in the file.
pub(super) fn extract_relationships(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let targets = Targets {
        custom_properties: symbols_by_metadata(symbols, "property"),
        keyframes: symbols_by_metadata(symbols, "animationName"),
        class_rules: class_rules_by_name(symbols),
    };
    let mut relationships = Vec::new();
    let mut seen = HashSet::new();
    let mut references = Vec::new();
    collect_references(base, tree.root_node(), &targets, &mut references, 0);
    for (node, candidates, reference_type) in references {
        let site = ReferenceSite {
            name: base.get_node_text(&node),
            span: NormalizedSpan::from_node(&node),
            kind: RelationshipKind::References,
            reference_type,
        };
        for target in candidates {
            push_relationship(base, symbols, target, &site, &mut seen, &mut relationships);
        }
    }
    push_custom_function_calls(base, symbols, &mut seen, &mut relationships);
    relationships
}

/// One `calls` edge per custom function call (`--name(...)`) to each
/// `@function` declaration of that name in the file.
fn push_custom_function_calls(
    base: &BaseExtractor,
    symbols: &[Symbol],
    seen: &mut HashSet<(String, String, u32, String)>,
    relationships: &mut Vec<Relationship>,
) {
    let mut declarations: HashMap<&str, Vec<&Symbol>> = HashMap::new();
    for symbol in symbols.iter().filter(|symbol| is_custom_function(symbol)) {
        declarations
            .entry(symbol.name.as_str())
            .or_default()
            .push(symbol);
    }
    if declarations.is_empty() {
        return;
    }
    for call in dashed_functions(&base.content) {
        let Some(targets) = declarations
            .get(call.name.as_str())
            .filter(|_| !call.is_declaration)
        else {
            continue;
        };
        let Some(span) = NormalizedSpan::from_content_range_with_line_starts(
            &base.content,
            base.line_starts(),
            call.start_byte,
            call.end_byte,
        ) else {
            continue;
        };
        let site = ReferenceSite {
            name: call.name.clone(),
            span,
            kind: RelationshipKind::Calls,
            reference_type: "custom-function",
        };
        for target in targets {
            push_relationship(base, symbols, target, &site, seen, relationships);
        }
    }
}

fn is_custom_function(symbol: &Symbol) -> bool {
    symbol.kind == SymbolKind::Function
        && symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("atRuleType"))
            .and_then(Value::as_str)
            == Some("function")
}

struct ReferenceSite {
    name: String,
    span: NormalizedSpan,
    kind: RelationshipKind,
    reference_type: &'static str,
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
        "declaration" => {
            let composed = composed_class_nodes(base, node);
            let is_local = composed
                .last()
                .and_then(|last| last.next_named_sibling())
                .is_none_or(|next| base.get_node_text(&next) != "from");
            for class_node in composed.into_iter().filter(|_| is_local) {
                if let Some(candidates) = targets.class_rules.get(&base.get_node_text(&class_node))
                {
                    references.push((class_node, candidates, "composes"));
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

fn innermost_symbol_containing(symbols: &[Symbol], span: NormalizedSpan) -> Option<&Symbol> {
    let (start, end) = (span.start_byte, span.end_byte);
    symbols
        .iter()
        .filter(|symbol| symbol.start_byte <= start && end <= symbol.end_byte)
        .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
}

/// Rules whose whole selector is one class (`.base`), keyed by class name.
fn class_rules_by_name(symbols: &[Symbol]) -> HashMap<String, Vec<&Symbol>> {
    let mut by_name: HashMap<String, Vec<&Symbol>> = HashMap::new();
    for (selector, symbol) in symbols_by_metadata(symbols, "selector") {
        if let Some(class) = selector.strip_prefix('.').filter(|class| {
            class
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        }) {
            by_name.entry(class.to_string()).or_default().extend(symbol);
        }
    }
    by_name
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
    site: &ReferenceSite,
    seen: &mut HashSet<(String, String, u32, String)>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(source) =
        innermost_symbol_containing(symbols, site.span).filter(|source| source.id != target.id)
    else {
        return;
    };
    let reference_name = site.name.clone();
    let span = site.span;
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
        Value::String(site.reference_type.to_string()),
    );

    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}_{}",
            source.id, target.id, site.kind, line_number, reference_name
        ),
        from_symbol_id: source.id.clone(),
        to_symbol_id: target.id.clone(),
        kind: site.kind.clone(),
        file_path: base.file_path.clone(),
        line_number,
        span: Some(span),
        reference_site_is_exact: true,
        confidence: 1.0,
        metadata: Some(metadata),
    });
}

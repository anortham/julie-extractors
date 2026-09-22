//! OpenAPI / Swagger operation route facts for JSON and YAML documents.
//!
//! Works over the extracted key symbols, so one collector serves both data
//! languages: a root `openapi` or `swagger` key marks the document, and each
//! `paths.<template>.<verb>` key becomes one `openapi.route.v1` fact.

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use super::span::NormalizedSpan;
use super::structural_fact_builders::{base_metadata, fact_for_span, insert_string};
use super::types::{StructuralFact, Symbol};

pub(crate) const OPENAPI_ROUTE_PATTERN_ID: &str = "openapi.route.v1";

const HTTP_VERBS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

pub(crate) fn collect_openapi_route_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let root_child = |name: &str| {
        symbols
            .iter()
            .find(|symbol| symbol.parent_id.is_none() && symbol.name == name)
    };
    let Some((spec_format, version_symbol)) = root_child("openapi")
        .map(|symbol| ("openapi", symbol))
        .or_else(|| root_child("swagger").map(|symbol| ("swagger", symbol)))
    else {
        return Vec::new();
    };
    let Some(paths) = root_child("paths") else {
        return Vec::new();
    };
    let spec_version = scalar_value(tree, content, version_symbol);
    let base_path = root_child("basePath").and_then(|symbol| scalar_value(tree, content, symbol));

    let mut facts = Vec::new();
    for path in children_of(symbols, paths).filter(|symbol| symbol.name.starts_with('/')) {
        for operation in children_of(symbols, path) {
            let verb = operation.name.to_ascii_lowercase();
            if !HTTP_VERBS.contains(&verb.as_str()) {
                continue;
            }
            let effective = match base_path.as_deref() {
                Some(base) if !base.is_empty() && base != "/" => {
                    join_route_templates(base, &path.name)
                }
                _ => path.name.clone(),
            };
            let normalized = normalize_route_template(&effective, ParamFlavor::Braces);

            let mut metadata = base_metadata("framework");
            insert_string(&mut metadata, "framework", "openapi");
            insert_string(&mut metadata, "spec_format", spec_format);
            if let Some(version) = &spec_version {
                insert_string(&mut metadata, "spec_version", version);
            }
            insert_string(&mut metadata, "verb", &verb.to_ascii_uppercase());
            insert_string(&mut metadata, "route_template", &path.name);
            if effective != path.name {
                insert_string(&mut metadata, "effective_route_template", &effective);
            }
            insert_string(
                &mut metadata,
                "normalized_route_template",
                &normalized.template,
            );
            if !normalized.dynamic_segments.is_empty() {
                metadata.insert(
                    "dynamic_segments".to_string(),
                    Value::Array(
                        normalized
                            .dynamic_segments
                            .into_iter()
                            .map(Value::String)
                            .collect(),
                    ),
                );
            }
            if let Some(operation_id) = children_of(symbols, operation)
                .find(|symbol| symbol.name == "operationId")
                .and_then(|symbol| scalar_value(tree, content, symbol))
            {
                insert_string(&mut metadata, "operation_id", &operation_id);
            }

            facts.push(fact_for_span(
                file_path,
                language,
                OPENAPI_ROUTE_PATTERN_ID,
                "route",
                "operation",
                symbol_span(operation),
                metadata,
            ));
        }
    }
    facts
}

fn children_of<'a>(symbols: &'a [Symbol], parent: &'a Symbol) -> impl Iterator<Item = &'a Symbol> {
    symbols
        .iter()
        .filter(move |symbol| symbol.parent_id.as_deref() == Some(parent.id.as_str()))
}

fn symbol_span(symbol: &Symbol) -> NormalizedSpan {
    NormalizedSpan {
        start_line: symbol.start_line,
        start_column: symbol.start_column,
        end_line: symbol.end_line,
        end_column: symbol.end_column,
        start_byte: symbol.start_byte,
        end_byte: symbol.end_byte,
    }
}

/// The unquoted scalar value of a key symbol's pair node (JSON `pair`, YAML
/// `block_mapping_pair` / `flow_pair`), or `None` for containers.
fn scalar_value(tree: &Tree, content: &str, symbol: &Symbol) -> Option<String> {
    let value = pair_node(tree, symbol)?.child_by_field_name("value")?;
    let text = |node: Node<'_>| content.get(node.start_byte()..node.end_byte());
    if value.kind() == "string" {
        return serde_json::from_str::<String>(text(value)?).ok();
    }
    let mut cursor = value.walk();
    let scalar = value.named_children(&mut cursor).last()?;
    match scalar.kind() {
        "plain_scalar" => Some(text(scalar)?.trim().to_string()),
        "double_quote_scalar" | "single_quote_scalar" => {
            Some(text(scalar)?.trim_matches(['"', '\'']).to_string())
        }
        _ => None,
    }
}

fn pair_node<'tree>(tree: &'tree Tree, symbol: &Symbol) -> Option<Node<'tree>> {
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)?;
    while node.start_byte() != symbol.start_byte as usize
        || node.end_byte() != symbol.end_byte as usize
        || node.child_by_field_name("value").is_none()
    {
        node = node.parent()?;
    }
    Some(node)
}

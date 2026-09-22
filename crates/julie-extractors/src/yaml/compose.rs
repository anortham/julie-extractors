//! Docker Compose semantics, gated on the compose file names.
//!
//! Each `services.<name>` entry is a `yaml.compose_service.v1` fact.
//! `depends_on`, `extends.service` without a file, and named volume uses are
//! `References` edges to the service or top-level volume in the same file.
//! `extends.file` is a structured pending row to the other file, with the
//! service as its terminal name.

use super::innermost_symbol;
use super::relationships::{
    document_roots, field, file_name, mapping_pairs, pair_key, push_pending, push_reference,
    scalar_list, scalar_value, sequence_items, symbol_for_node,
};
use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};
use crate::base::{BaseExtractor, Relationship, StructuralFact, Symbol, UnresolvedTarget};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

pub(crate) const COMPOSE_SERVICE_PATTERN_ID: &str = "yaml.compose_service.v1";

fn is_compose_path(file_path: &str) -> bool {
    let name = file_name(file_path);
    let is_yaml = name.ends_with(".yml") || name.ends_with(".yaml");
    is_yaml && (name.starts_with("docker-compose") || name.starts_with("compose."))
}

/// `(name, pair, value)` for each service, and the top-level volume pairs.
fn sections<'tree>(
    content: &str,
    tree: &'tree Tree,
) -> (Vec<(String, Node<'tree>, Node<'tree>)>, Vec<Node<'tree>>) {
    let mut services = Vec::new();
    let mut volumes = Vec::new();
    for root in document_roots(tree) {
        for pair in mapping_pairs(root) {
            let (Some(key), Some(value)) =
                (pair_key(content, pair), pair.child_by_field_name("value"))
            else {
                continue;
            };
            match key.as_str() {
                "services" => {
                    services.extend(mapping_pairs(value).into_iter().filter_map(|service| {
                        Some((
                            pair_key(content, service)?,
                            service,
                            service.child_by_field_name("value")?,
                        ))
                    }))
                }
                "volumes" => volumes.extend(mapping_pairs(value)),
                _ => {}
            }
        }
    }
    (services, volumes)
}

pub(crate) fn compose_facts(tree: &Tree, file_path: &str, content: &str) -> Vec<StructuralFact> {
    if !is_compose_path(file_path) {
        return Vec::new();
    }
    sections(content, tree)
        .0
        .into_iter()
        .map(|(name, pair, value)| service_fact(file_path, content, &name, pair, value))
        .collect()
}

#[inline(never)]
fn service_fact(
    file_path: &str,
    content: &str,
    name: &str,
    pair: Node,
    value: Node,
) -> StructuralFact {
    let mut metadata = base_metadata("service_structure");
    insert_string(&mut metadata, "name", name);
    let text = |key: &str| {
        field(content, value, key)
            .and_then(|node| scalar_value(content, node).map(|(_, text)| text))
    };
    if let Some(image) = text("image") {
        insert_string(&mut metadata, "image", &image);
    }
    let build = field(content, value, "build");
    let context = build.and_then(|build| {
        scalar_value(content, build)
            .map(|(_, text)| text)
            .or_else(|| {
                field(content, build, "context")
                    .and_then(|node| scalar_value(content, node).map(|(_, text)| text))
            })
    });
    if let Some(context) = context {
        insert_string(&mut metadata, "build_context", &context);
    }
    let ports: Vec<Value> = field(content, value, "ports")
        .map(|ports| {
            scalar_list(content, ports)
                .into_iter()
                .map(|(_, text)| Value::String(text))
                .collect()
        })
        .unwrap_or_default();
    if !ports.is_empty() {
        metadata.insert("ports".to_string(), Value::Array(ports));
    }
    fact_for_node(
        file_path,
        "yaml",
        COMPOSE_SERVICE_PATTERN_ID,
        "service",
        pair,
        metadata,
    )
}

pub(super) fn extract_relationships(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    if !is_compose_path(&base.file_path) {
        return;
    }
    let content = base.content.clone();
    let (services, volumes) = sections(&content, tree);
    let service_symbols: HashMap<String, &Symbol> = services
        .iter()
        .filter_map(|(name, pair, _)| Some((name.clone(), symbol_for_node(symbols, *pair)?)))
        .collect();
    let volume_symbols: HashMap<String, &Symbol> = volumes
        .iter()
        .filter_map(|pair| Some((pair_key(&content, *pair)?, symbol_for_node(symbols, *pair)?)))
        .collect();
    let link =
        |relationships: &mut Vec<Relationship>, site: Node, target: Option<&&Symbol>, kind| {
            if let (Some(from), Some(to)) = (innermost_symbol(symbols, site), target) {
                push_reference(base, from, to, &site, ("composeRef", kind), relationships);
            }
        };
    for (_, _, service) in &services {
        if let Some(depends_on) = field(&content, *service, "depends_on") {
            for (site, name) in scalar_list(&content, depends_on) {
                link(
                    relationships,
                    site,
                    service_symbols.get(&name),
                    "depends_on",
                );
            }
            for pair in mapping_pairs(depends_on) {
                if let (Some(name), Some(key)) =
                    (pair_key(&content, pair), pair.child_by_field_name("key"))
                {
                    link(relationships, key, service_symbols.get(&name), "depends_on");
                }
            }
        }
        if let Some(uses) = field(&content, *service, "volumes") {
            for item in sequence_items(uses) {
                let source = scalar_value(&content, item)
                    .map(|(node, text)| {
                        (node, text.split(':').next().unwrap_or_default().to_string())
                    })
                    .or_else(|| {
                        field(&content, item, "source")
                            .and_then(|node| scalar_value(&content, node))
                    });
                if let Some((site, name)) = source {
                    link(relationships, site, volume_symbols.get(&name), "volume");
                }
            }
        }
    }
    let mut pending = Vec::new();
    for (_, _, service) in &services {
        let Some(extends) = field(&content, *service, "extends") else {
            continue;
        };
        if let Some((site, name)) = scalar_value(&content, extends) {
            link(relationships, site, service_symbols.get(&name), "extends");
            continue;
        }
        let service_name =
            field(&content, extends, "service").and_then(|node| scalar_value(&content, node));
        let file = field(&content, extends, "file").and_then(|node| scalar_value(&content, node));
        match (file, service_name) {
            (Some((site, file)), service_name) => {
                let Some(from) = innermost_symbol(symbols, site) else {
                    continue;
                };
                let terminal_name = service_name
                    .map(|(_, name)| name)
                    .unwrap_or_else(|| file.clone());
                let target = UnresolvedTarget {
                    display_name: format!("{file}#{terminal_name}"),
                    terminal_name,
                    receiver: None,
                    namespace_path: vec!["services".to_string()],
                    import_context: Some(file),
                };
                pending.push((from.clone(), target, site));
            }
            (None, Some((site, name))) => {
                link(relationships, site, service_symbols.get(&name), "extends");
            }
            (None, None) => {}
        }
    }
    for (from, target, site) in pending {
        push_pending(base, &from, target, site);
    }
}

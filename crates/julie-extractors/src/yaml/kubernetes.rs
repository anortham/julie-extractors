//! Kubernetes manifests and Kustomize files.
//!
//! - A document whose root holds `apiVersion` and `kind` is a resource: one
//!   `yaml.k8s_resource.v1` fact. `configMapRef`, `configMapKeyRef`,
//!   `secretRef`, `secretKeyRef`, `configMap`, `secret`,
//!   `persistentVolumeClaim`, and `serviceAccountName` name another resource;
//!   when that resource is in the same file, a `References` edge runs from the
//!   naming key to the resource's `metadata.name`.
//! - A `kustomization.yaml` lists other files and directories under
//!   `resources`, `components`, `bases`, `crds`, `configurations`,
//!   `patchesStrategicMerge`, and `patches[].path`: one structured pending row
//!   each. Remote URLs are skipped.

use super::ci::visit_pairs;
use super::innermost_symbol;
use super::relationships::{
    document_roots, field, file_name, mapping_pairs, pair_key, push_file_pending, push_reference,
    scalar_list, scalar_value, sequence_items, symbol_for_node,
};
use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};
use crate::base::{BaseExtractor, Relationship, StructuralFact, Symbol};
use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

pub(crate) const K8S_RESOURCE_PATTERN_ID: &str = "yaml.k8s_resource.v1";

struct Resource<'tree> {
    document: Node<'tree>,
    api_version: String,
    kind: String,
    name: Option<(Node<'tree>, String)>,
    namespace: Option<String>,
}

fn resources<'tree>(content: &str, tree: &'tree Tree) -> Vec<Resource<'tree>> {
    document_roots(tree)
        .into_iter()
        .filter_map(|root| {
            let text = |value: Option<Node>| {
                value
                    .and_then(|node| scalar_value(content, node))
                    .map(|(_, text)| text)
            };
            let metadata = field(content, root, "metadata");
            let name = metadata
                .and_then(|metadata| {
                    mapping_pairs(metadata)
                        .into_iter()
                        .find(|pair| pair_key(content, *pair).as_deref() == Some("name"))
                })
                .and_then(|pair| {
                    let value = pair.child_by_field_name("value")?;
                    Some((pair, scalar_value(content, value)?.1))
                });
            Some(Resource {
                document: root.parent()?,
                api_version: text(field(content, root, "apiVersion"))?,
                kind: text(field(content, root, "kind"))?,
                name,
                namespace: text(
                    metadata.and_then(|metadata| field(content, metadata, "namespace")),
                ),
            })
        })
        .collect()
}

pub(crate) fn k8s_facts(tree: &Tree, file_path: &str, content: &str) -> Vec<StructuralFact> {
    let resources = resources(content, tree);
    let roots = document_roots(tree);
    let multi = roots.len() > 1;
    resources
        .iter()
        .map(|resource| {
            let mut metadata = base_metadata("service_structure");
            insert_string(&mut metadata, "api_version", &resource.api_version);
            insert_string(&mut metadata, "kind", &resource.kind);
            if let Some((_, name)) = &resource.name {
                insert_string(&mut metadata, "name", name);
            }
            if let Some(namespace) = &resource.namespace {
                insert_string(&mut metadata, "namespace", namespace);
            }
            if multi
                && let Some(index) = roots
                    .iter()
                    .position(|root| root.parent().map(|d| d.id()) == Some(resource.document.id()))
            {
                metadata.insert(
                    "document_index".to_string(),
                    Value::Number(Number::from(index)),
                );
            }
            fact_for_node(
                file_path,
                "yaml",
                K8S_RESOURCE_PATTERN_ID,
                "resource",
                resource.document,
                metadata,
            )
        })
        .collect()
}

/// The resource kind a reference key names, and the field that holds the name.
fn reference_kind(key: &str) -> Option<(&'static str, Option<&'static str>)> {
    match key {
        "configMapRef" | "configMapKeyRef" | "configMap" => Some(("ConfigMap", Some("name"))),
        "secretRef" | "secretKeyRef" => Some(("Secret", Some("name"))),
        "secret" => Some(("Secret", Some("secretName"))),
        "persistentVolumeClaim" => Some(("PersistentVolumeClaim", Some("claimName"))),
        "serviceAccountName" => Some(("ServiceAccount", None)),
        _ => None,
    }
}

pub(super) fn extract_relationships(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    if is_kustomization_path(&base.file_path) {
        extract_kustomize_pending(base, tree, symbols);
        return;
    }
    let content = base.content.clone();
    let resources = resources(&content, tree);
    if resources.is_empty() {
        return;
    }
    let mut sites: Vec<(Node, String, &'static str)> = Vec::new();
    visit_pairs(tree.root_node(), 0, &mut |pair| {
        let Some((kind, name_field)) = pair_key(&content, pair).as_deref().and_then(reference_kind)
        else {
            return;
        };
        let Some(value) = pair.child_by_field_name("value") else {
            return;
        };
        let site = match name_field {
            Some(name_field) => {
                field(&content, value, name_field).and_then(|node| scalar_value(&content, node))
            }
            None => scalar_value(&content, value),
        };
        if let Some((node, name)) = site {
            sites.push((node, name, kind));
        }
    });
    for (site, name, kind) in sites {
        let target = resources
            .iter()
            .find(|resource| {
                resource.kind == kind && resource.name.as_ref().map(|(_, n)| n) == Some(&name)
            })
            .and_then(|resource| symbol_for_node(symbols, resource.name.as_ref()?.0));
        if let (Some(from), Some(to)) = (innermost_symbol(symbols, site), target) {
            push_reference(base, from, to, &site, ("k8sRef", kind), relationships);
        }
    }
}

fn is_kustomization_path(file_path: &str) -> bool {
    matches!(
        file_name(file_path),
        "kustomization.yaml" | "kustomization.yml" | "Kustomization"
    )
}

fn extract_kustomize_pending(base: &mut BaseExtractor, tree: &Tree, symbols: &[Symbol]) {
    let content = base.content.clone();
    let mut sites: Vec<(Node, String)> = Vec::new();
    for root in document_roots(tree) {
        for pair in mapping_pairs(root) {
            let (Some(key), Some(value)) =
                (pair_key(&content, pair), pair.child_by_field_name("value"))
            else {
                continue;
            };
            match key.as_str() {
                "resources"
                | "components"
                | "bases"
                | "crds"
                | "configurations"
                | "patchesStrategicMerge" => sites.extend(scalar_list(&content, value)),
                "patches" => sites.extend(sequence_items(value).into_iter().filter_map(|item| {
                    field(&content, item, "path").and_then(|node| scalar_value(&content, node))
                })),
                _ => {}
            }
        }
    }
    for (site, path) in sites {
        if path.contains("://") || path.starts_with("github.com/") || path.starts_with("git@") {
            continue;
        }
        if let Some(from) = innermost_symbol(symbols, site) {
            let from = from.clone();
            push_file_pending(base, &from, &path, &path, site);
        }
    }
}

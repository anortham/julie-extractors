//! Go checksum file (`go.sum`) facts.
//!
//! Each line records the hash the go command verified for one module version:
//! `<module> <version>[/go.mod] h1:<hash>`. A `go.sum` file declares nothing,
//! so it publishes no symbols or edges. Every line that parses cleanly is one
//! `gosum.checksum.v1` structural fact.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use tree_sitter::{Node, Tree};

use crate::base::StructuralFact;
use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};

pub(crate) const CHECKSUM_PATTERN_ID: &str = "gosum.checksum.v1";

/// `pseudoVersionRE` from `golang.org/x/mod/module`, with the commit time and
/// revision captured.
static PSEUDO_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^v[0-9]+\.(?:0\.0-|[0-9]+\.[0-9]+-(?:[^+]*\.)?0\.)(?P<timestamp>[0-9]{14})-(?P<revision>[A-Za-z0-9]+)(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$",
    )
    .expect("pseudo-version pattern compiles")
});

pub(crate) fn structural_facts(tree: &Tree, file_path: &str, content: &str) -> Vec<StructuralFact> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|node| node.kind() == "checksum" && !node.has_error())
        .filter_map(|node| checksum_fact(node, file_path, content))
        .collect()
}

fn checksum_fact(node: Node<'_>, file_path: &str, content: &str) -> Option<StructuralFact> {
    let value = child(node, "checksum_value")?;
    let version = text(child(node, "version")?, content)?;
    let mut metadata = base_metadata("dependencies");
    insert_string(
        &mut metadata,
        "module_path",
        text(child(node, "module_path")?, content)?,
    );
    insert_string(&mut metadata, "version", version);
    insert_string(
        &mut metadata,
        "hash_algorithm",
        text(child(value, "hash_version")?, content)?,
    );
    insert_string(&mut metadata, "hash", text(child(value, "hash")?, content)?);
    let flags = [
        ("go_mod", child(node, "go.mod").is_some()),
        ("incompatible", version.ends_with("+incompatible")),
    ];
    for (key, flag) in flags {
        metadata.insert(key.to_string(), Value::Bool(flag));
    }
    let pseudo = PSEUDO_VERSION.captures(version);
    metadata.insert("pseudo_version".to_string(), Value::Bool(pseudo.is_some()));
    if let Some(pseudo) = pseudo {
        insert_string(&mut metadata, "timestamp", &pseudo["timestamp"]);
        insert_string(&mut metadata, "revision", &pseudo["revision"]);
    }
    Some(fact_for_node(
        file_path,
        "gosum",
        CHECKSUM_PATTERN_ID,
        "checksum",
        node,
        metadata,
    ))
}

fn child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn text<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

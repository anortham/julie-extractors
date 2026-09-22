//! TOML domain-relationship extraction (Phase 3.3).
//!
//! TOML has no inter-key reference construct in the format itself; the
//! Phase 3.3 contract is *domain-aware*. Two file basenames trigger
//! relationship extraction; everything else emits nothing.
//!
//! - **Cargo.toml**: `[dependencies]`, `[dev-dependencies]`,
//!   `[build-dependencies]`, and target-scoped `[target.<triple>.dependencies]`
//!   tables emit `RelationshipKind::Imports` edges from the table symbol to
//!   each child key (e.g., `serde`, `tokio`).
//! - **pyproject.toml**: `[tool.<x>.*]` tables emit
//!   `RelationshipKind::References` edges, one per unique top-level tool
//!   name. `[tool.pytest]` and `[tool.pytest.ini_options]` collapse to a
//!   single `pytest` edge.
//!
//! Other tables — including dotted tables in non-Cargo / non-pyproject
//! files — produce no relationships. Symbol extraction is unchanged.

use super::dependencies::{
    Manifest, collect_dependencies, header_parts, pair_key_parts, pairs, string_value,
};
use super::pair_value;
use crate::base::{
    BaseExtractor, Relationship, RelationshipKind, StructuredPendingRelationship, Symbol,
    UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) fn extract_relationships_internal(
    base: &mut BaseExtractor,
    root: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(manifest) = Manifest::for_path(&base.file_path) else {
        return;
    };
    extract_dependency_imports(base, manifest, root, symbols, relationships);
    match manifest {
        Manifest::Cargo => extract_cargo_workspace_inheritance(base, root, symbols),
        Manifest::Pyproject => {
            extract_pyproject_relationships(base, root, symbols, relationships);
            extract_entry_point_pending(base, root, symbols);
        }
    }
}

fn walk_tables<F: FnMut(Node)>(node: Node, mut f: F) {
    fn recurse<F: FnMut(Node)>(node: Node, f: &mut F, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        if matches!(node.kind(), "table" | "table_array_element") {
            f(node);
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        for child in node.children(&mut node.walk()) {
            recurse(child, f, child_depth);
        }
    }
    recurse(node, &mut f, 0);
}

fn symbol_at<'a>(symbols: &'a [Symbol], node: Node) -> Option<&'a Symbol> {
    symbols
        .iter()
        .find(|s| s.start_byte == node.start_byte() as u32)
}

fn root_symbol<'a>(symbols: &'a [Symbol], names: &[&str]) -> Option<&'a Symbol> {
    names.iter().find_map(|name| {
        symbols
            .iter()
            .find(|s| s.parent_id.is_none() && s.name == *name)
    })
}

/// `Imports` edges for dependencies that are key symbols: a pair in a Cargo or
/// Poetry dependency table (edge from the table), or a Cargo
/// `[dependencies.<name>]` table (edge from the manifest's `[package]` or
/// `[workspace]` table). PEP 508 requirement strings have no symbol and appear
/// only as `manifest.dependency.v1` facts.
fn extract_dependency_imports(
    base: &BaseExtractor,
    manifest: Manifest,
    root: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    for dependency in collect_dependencies(manifest, root, &base.content) {
        let Some(dep_symbol) = symbol_at(symbols, dependency.node) else {
            continue;
        };
        let from_symbol = match dependency.table {
            Some(table) => symbol_at(symbols, table),
            None if dependency.node.kind() == "table" => {
                root_symbol(symbols, &["package", "workspace"])
            }
            None => None,
        };
        let Some(from_symbol) = from_symbol else {
            continue;
        };
        let mut metadata = HashMap::new();
        metadata.insert(
            "dependencyKind".to_string(),
            Value::String(dependency.group.clone()),
        );
        metadata.insert(
            "dependencyName".to_string(),
            Value::String(dependency.name.clone()),
        );
        for (key, value) in [
            ("target", &dependency.target),
            ("package", &dependency.package),
        ] {
            if let Some(value) = value {
                metadata.insert(key.to_string(), Value::String(value.clone()));
            }
        }
        if dependency.inherited {
            metadata.insert("workspace".to_string(), Value::Bool(true));
        }
        let node = dependency.node;
        relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                from_symbol.id,
                dep_symbol.id,
                RelationshipKind::Imports,
                node.start_position().row
            ),
            from_symbol_id: from_symbol.id.clone(),
            to_symbol_id: dep_symbol.id.clone(),
            kind: RelationshipKind::Imports,
            file_path: base.file_path.clone(),
            line_number: node.start_position().row as u32 + 1,
            span: Some(crate::base::NormalizedSpan::from_node(&node)),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: Some(metadata),
        });
    }
}

fn extract_pyproject_relationships(
    base: &BaseExtractor,
    root: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let mut seen_tools: HashSet<String> = HashSet::new();
    walk_tables(root, |table| {
        let name = match table_header_text(base, table) {
            Some(n) => n,
            None => return,
        };
        let segments: Vec<&str> = name.split('.').collect();
        if segments.len() < 2 || segments[0] != "tool" {
            return;
        }
        let tool_name = segments[1].to_string();
        if !seen_tools.insert(tool_name.clone()) {
            return;
        }
        let table_symbol = match symbols
            .iter()
            .find(|s| s.start_byte == table.start_byte() as u32)
        {
            Some(s) => s,
            None => return,
        };
        // From-side: prefer a top-level `[project]` table symbol if present,
        // otherwise fall back to the table itself (the resolver can still
        // route on metadata.toolName).
        let from_id = symbols
            .iter()
            .find(|s| s.name == "project" && s.parent_id.is_none())
            .map(|s| s.id.clone())
            .unwrap_or_else(|| table_symbol.id.clone());
        let mut metadata = HashMap::new();
        metadata.insert("toolName".to_string(), Value::String(tool_name.clone()));
        relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                from_id,
                table_symbol.id,
                RelationshipKind::References,
                table.start_position().row
            ),
            from_symbol_id: from_id,
            to_symbol_id: table_symbol.id.clone(),
            kind: RelationshipKind::References,
            file_path: base.file_path.clone(),
            line_number: table.start_position().row as u32 + 1,
            span: Some(crate::base::NormalizedSpan::from_node(&table)),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: Some(metadata),
        });
    });
}

/// Cargo workspace inheritance (`serde = { workspace = true }`,
/// `version.workspace = true`, `[lints] workspace = true`) names a key in the
/// workspace root manifest, so each one is a pending reference to that key.
fn extract_cargo_workspace_inheritance(base: &mut BaseExtractor, root: Node, symbols: &[Symbol]) {
    let content = base.content.clone();
    let mut targets: Vec<(Node, Vec<String>, String)> = Vec::new();
    for dependency in collect_dependencies(Manifest::Cargo, root, &content) {
        if dependency.inherited {
            targets.push((
                dependency.node,
                vec!["workspace".to_string(), "dependencies".to_string()],
                dependency.name,
            ));
        }
    }
    let mut cursor = root.walk();
    for table in root
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "table")
    {
        let header = header_parts(table, &content).unwrap_or_default();
        for pair in pairs(table) {
            let (Some(key), Some(value)) = (pair_key_parts(pair, &content), pair_value(pair))
            else {
                continue;
            };
            let is_true = |node: Node| {
                node.kind() == "boolean"
                    && content.get(node.start_byte()..node.end_byte()) == Some("true")
            };
            let inherits = match key.as_slice() {
                [_, field] => field == "workspace" && is_true(value),
                [_] => pairs(value).into_iter().any(|field| {
                    pair_key_parts(field, &content).as_deref() == Some(&["workspace".to_string()])
                        && pair_value(field).is_some_and(is_true)
                }),
                _ => false,
            };
            match header.as_slice() {
                [section] if section == "package" && inherits => targets.push((
                    pair,
                    vec!["workspace".to_string(), "package".to_string()],
                    key[0].clone(),
                )),
                [section] if section == "lints" && key == ["workspace"] && is_true(value) => {
                    targets.push((pair, vec!["workspace".to_string()], "lints".to_string()));
                }
                _ => {}
            }
        }
    }

    for (node, namespace_path, terminal_name) in targets {
        let Some(from_symbol) = symbol_at(symbols, node) else {
            continue;
        };
        let target = UnresolvedTarget {
            display_name: format!("{}.{terminal_name}", namespace_path.join(".")),
            terminal_name,
            receiver: None,
            namespace_path,
            import_context: Some("Cargo.toml".to_string()),
        };
        push_pending(base, from_symbol, target, node);
    }
}

/// Python entry points (`[project.scripts]`, `[project.gui-scripts]`,
/// `[project.entry-points.<group>]`, `[tool.poetry.scripts]`) name a callable
/// as `package.module:attr` in another file: one pending reference each.
fn extract_entry_point_pending(base: &mut BaseExtractor, root: Node, symbols: &[Symbol]) {
    let content = base.content.clone();
    let mut cursor = root.walk();
    for table in root
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "table")
    {
        let header = header_parts(table, &content).unwrap_or_default();
        let header: Vec<&str> = header.iter().map(String::as_str).collect();
        if !matches!(
            header.as_slice(),
            ["project", "scripts" | "gui-scripts"]
                | ["project", "entry-points", _]
                | ["tool", "poetry", "scripts"]
        ) {
            continue;
        }
        for pair in pairs(table) {
            let Some(reference) = pair_value(pair).and_then(|value| string_value(value, &content))
            else {
                continue;
            };
            let Some(target) = entry_point_target(&reference) else {
                continue;
            };
            let Some(from_symbol) = symbol_at(symbols, pair) else {
                continue;
            };
            push_pending(base, from_symbol, target, pair);
        }
    }
}

/// `acme.cli:main` -> terminal `main`, namespace `[acme, cli]`, context `acme.cli`.
fn entry_point_target(reference: &str) -> Option<UnresolvedTarget> {
    let reference = reference.split('[').next()?.trim();
    let (module, attr) = match reference.split_once(':') {
        Some((module, attr)) => (module.trim(), Some(attr.trim())),
        None => (reference, None),
    };
    if module.is_empty() || module.contains(char::is_whitespace) {
        return None;
    }
    let mut path: Vec<String> = module.split('.').map(str::to_string).collect();
    if let Some(attr) = attr.filter(|attr| !attr.is_empty()) {
        path.extend(attr.split('.').map(str::to_string));
    }
    let terminal_name = path.pop()?;
    Some(UnresolvedTarget {
        display_name: reference.to_string(),
        terminal_name,
        receiver: None,
        namespace_path: path,
        import_context: Some(module.to_string()),
    })
}

fn push_pending(
    base: &mut BaseExtractor,
    from_symbol: &Symbol,
    target: UnresolvedTarget,
    node: Node,
) {
    let pending = StructuredPendingRelationship::new(
        from_symbol.id.clone(),
        target,
        Some(from_symbol.id.clone()),
        RelationshipKind::References,
        base.file_path.clone(),
        node.start_position().row as u32 + 1,
        1.0,
    );
    base.add_structured_pending_relationship(pending);
}

/// Extract the textual header of a `table` node (the part between `[` and
/// `]`). Returns the dotted form (e.g., `"tool.pytest.ini_options"`,
/// `"target.x86_64-unknown-linux-gnu.dependencies"`).
fn table_header_text(base: &BaseExtractor, table: Node) -> Option<String> {
    for child in table.children(&mut table.walk()) {
        match child.kind() {
            "bare_key" | "quoted_key" => {
                let raw = base.get_node_text(&child);
                return Some(raw.trim_matches('"').trim_matches('\'').to_string());
            }
            "dotted_key" => {
                return Some(base.get_node_text(&child));
            }
            _ => {}
        }
    }
    None
}

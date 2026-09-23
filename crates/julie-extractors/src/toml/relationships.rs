//! TOML domain-relationship extraction (Phase 3.3).
//!
//! TOML has no inter-key reference construct in the format itself; the
//! contract is *domain-aware*. File basenames trigger relationship
//! extraction; everything else emits nothing.
//!
//! - **Cargo.toml**: `[dependencies]`, `[dev-dependencies]`,
//!   `[build-dependencies]`, and target-scoped `[target.<triple>.dependencies]`
//!   tables emit `RelationshipKind::Imports` edges from the table symbol to
//!   each child key (e.g., `serde`, `tokio`). `[features]` entries and
//!   `required-features` lists emit `References` edges to the features and
//!   dependencies they name (`std`, `dep:serde_json`, `serde?/std`).
//! - **pyproject.toml**: `[tool.<x>.*]` tables emit
//!   `RelationshipKind::References` edges, one per unique top-level tool
//!   name, from `[project]` (else `[tool.poetry]`). `[tool.pytest]` and
//!   `[tool.pytest.ini_options]` collapse to a single `pytest` edge.
//! - **Pipfile**: `[packages]` and `[dev-packages]` emit `Imports` edges.
//! - **Gradle version catalogs** (`*.versions.toml`): `version.ref` entries
//!   reference `[versions]` keys and `[bundles]` lists reference
//!   `[libraries]` aliases.
//!
//! Other tables produce no relationships. Symbol extraction is unchanged.

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
    if is_version_catalog(&base.file_path) {
        extract_version_catalog_references(base, root, symbols, relationships);
        return;
    }
    let Some(manifest) = Manifest::for_path(&base.file_path) else {
        return;
    };
    extract_dependency_imports(base, manifest, root, symbols, relationships);
    match manifest {
        Manifest::Cargo => {
            extract_cargo_workspace_inheritance(base, root, symbols);
            extract_cargo_feature_references(base, root, symbols, relationships);
        }
        Manifest::Pyproject => {
            extract_pyproject_relationships(base, root, symbols, relationships);
            extract_entry_point_pending(base, root, symbols);
        }
        Manifest::Pipfile => {}
    }
}

fn root_tables<'tree>(root: Node<'tree>, content: &str) -> Vec<(Vec<String>, Node<'tree>)> {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|node| matches!(node.kind(), "table" | "table_array_element"))
        .filter_map(|table| Some((header_parts(table, content)?, table)))
        .collect()
}

fn string_items<'tree>(value: Node<'tree>, content: &str) -> Vec<(String, Node<'tree>)> {
    if value.kind() != "array" {
        return Vec::new();
    }
    let mut cursor = value.walk();
    value
        .named_children(&mut cursor)
        .filter_map(|item| Some((string_value(item, content)?, item)))
        .collect()
}

#[inline(never)]
fn push_reference(
    base: &BaseExtractor,
    from: &Symbol,
    to: &Symbol,
    site: Node,
    metadata: HashMap<String, Value>,
    relationships: &mut Vec<Relationship>,
) {
    if from.id == to.id {
        return;
    }
    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}_{}",
            from.id,
            to.id,
            RelationshipKind::References,
            site.start_position().row,
            site.start_position().column
        ),
        from_symbol_id: from.id.clone(),
        to_symbol_id: to.id.clone(),
        kind: RelationshipKind::References,
        file_path: base.file_path.clone(),
        line_number: site.start_position().row as u32 + 1,
        span: Some(crate::base::NormalizedSpan::from_node(&site)),
        reference_site_is_exact: false,
        confidence: 1.0,
        metadata: Some(metadata),
    });
}

/// Cargo `[features]` entries and `required-features` lists name features in
/// the same manifest (`default = ["std"]`) or dependencies (`dep:serde_json`,
/// `serde/std`, `serde?/std`, and an optional dependency's implicit feature).
fn extract_cargo_feature_references(
    base: &BaseExtractor,
    root: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let content = &base.content;
    let mut dependencies: HashMap<String, &Symbol> = HashMap::new();
    for dependency in collect_dependencies(Manifest::Cargo, root, content) {
        let Some(symbol) = symbol_at(symbols, dependency.node) else {
            continue;
        };
        if dependency.group == "dependencies" || !dependencies.contains_key(&dependency.name) {
            dependencies.insert(dependency.name.clone(), symbol);
        }
    }
    let tables = root_tables(root, content);
    let mut features: HashMap<String, &Symbol> = HashMap::new();
    let mut lists: Vec<Node> = Vec::new();
    for (header, table) in &tables {
        let is_features = header.as_slice() == ["features".to_string()];
        let is_target = matches!(
            header.as_slice(),
            [kind] if matches!(kind.as_str(), "lib" | "bin" | "example" | "test" | "bench")
        );
        for pair in pairs(*table) {
            let Some(key) = pair_key_parts(pair, content) else {
                continue;
            };
            if is_features && key.len() == 1 {
                if let Some(symbol) = symbol_at(symbols, pair) {
                    features.insert(key[0].clone(), symbol);
                }
                lists.push(pair);
            } else if is_target && key == ["required-features"] {
                lists.push(pair);
            }
        }
    }
    for pair in lists {
        let (Some(from), Some(value)) = (symbol_at(symbols, pair), pair_value(pair)) else {
            continue;
        };
        for (item, site) in string_items(value, content) {
            let target = if let Some(name) = item.strip_prefix("dep:") {
                dependencies.get(name)
            } else if let Some((name, _)) = item.split_once('/') {
                dependencies.get(name.trim_end_matches('?'))
            } else {
                features.get(&item).or_else(|| dependencies.get(&item))
            };
            if let Some(target) = target {
                let metadata =
                    HashMap::from([("cargoFeature".to_string(), Value::String(item.clone()))]);
                push_reference(base, from, target, site, metadata, relationships);
            }
        }
    }
}

fn is_version_catalog(file_path: &str) -> bool {
    file_path
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| name.ends_with(".versions.toml"))
}

/// Gradle version catalogs: `version.ref = "x"` (or `version = { ref = "x" }`)
/// in `[libraries]` and `[plugins]` names a `[versions]` key, and each
/// `[bundles]` entry lists `[libraries]` aliases.
fn extract_version_catalog_references(
    base: &BaseExtractor,
    root: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let content = &base.content;
    let tables = root_tables(root, content);
    let entries = |section: &str| -> Vec<(String, Node)> {
        tables
            .iter()
            .filter(|(header, _)| header.as_slice() == [section.to_string()])
            .flat_map(|(_, table)| pairs(*table))
            .filter_map(|pair| Some((pair_key_parts(pair, content)?.join("."), pair)))
            .collect()
    };
    let named = |section: &str| -> HashMap<String, &Symbol> {
        entries(section)
            .into_iter()
            .filter_map(|(name, pair)| Some((catalog_alias(&name), symbol_at(symbols, pair)?)))
            .collect()
    };
    let versions = named("versions");
    let libraries = named("libraries");
    for (_, entry) in entries("libraries").into_iter().chain(entries("plugins")) {
        let Some(value) = pair_value(entry).filter(|value| value.kind() == "inline_table") else {
            continue;
        };
        for field in pairs(value) {
            let (Some(key), Some(field_value)) =
                (pair_key_parts(field, content), pair_value(field))
            else {
                continue;
            };
            let reference = match key.as_slice() {
                [version, reference] if version == "version" && reference == "ref" => Some(field),
                [version] if version == "version" && field_value.kind() == "inline_table" => {
                    pairs(field_value).into_iter().find(|inner| {
                        pair_key_parts(*inner, content).as_deref() == Some(&["ref".to_string()])
                    })
                }
                _ => None,
            };
            let Some(reference) = reference else {
                continue;
            };
            let (Some(from), Some(name)) = (
                symbol_at(symbols, reference),
                pair_value(reference).and_then(|v| string_value(v, content)),
            ) else {
                continue;
            };
            if let Some(target) = versions.get(&catalog_alias(&name)) {
                let metadata = HashMap::from([("versionRef".to_string(), Value::String(name))]);
                push_reference(base, from, target, reference, metadata, relationships);
            }
        }
    }
    for (_, bundle) in entries("bundles") {
        let (Some(from), Some(value)) = (symbol_at(symbols, bundle), pair_value(bundle)) else {
            continue;
        };
        for (alias, site) in string_items(value, content) {
            if let Some(target) = libraries.get(&catalog_alias(&alias)) {
                let metadata = HashMap::from([("bundleLibrary".to_string(), Value::String(alias))]);
                push_reference(base, from, target, site, metadata, relationships);
            }
        }
    }
}

/// Gradle treats `-`, `_`, and `.` in catalog aliases as the same separator.
fn catalog_alias(name: &str) -> String {
    name.replace(['_', '.'], "-")
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
        let name = match super::header_name(table, &base.content) {
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
        // From-side: the project descriptor, `[project]` or else a Poetry
        // project's `[tool.poetry]`. A table never references itself.
        let Some(from_id) = root_symbol(symbols, &["project", "tool.poetry"])
            .map(|s| s.id.clone())
            .filter(|id| *id != table_symbol.id)
        else {
            return;
        };
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

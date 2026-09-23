//! Package-manifest dependency declarations in `Cargo.toml`, `pyproject.toml`,
//! and `Pipfile`.
//!
//! One parse feeds two outputs: `Imports` edges for dependencies that are key
//! symbols (Cargo and Poetry tables) and `manifest.dependency.v1` facts for
//! every dependency, including PEP 508 requirement strings that have no symbol.

use tree_sitter::Node;

use super::pair_value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Manifest {
    Cargo,
    Pyproject,
    Pipfile,
}

impl Manifest {
    pub(crate) fn for_path(file_path: &str) -> Option<Self> {
        match file_path.rsplit(['/', '\\']).next()? {
            "Cargo.toml" => Some(Self::Cargo),
            "pyproject.toml" => Some(Self::Pyproject),
            "Pipfile" => Some(Self::Pipfile),
            _ => None,
        }
    }

    pub(crate) fn ecosystem(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Pyproject | Self::Pipfile => "pypi",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Dependency<'tree> {
    pub name: String,
    /// The pair, table, or requirement-string node that declares the dependency.
    pub node: Node<'tree>,
    /// The dependency table that holds a pair-form dependency.
    pub table: Option<Node<'tree>>,
    pub group: String,
    pub version: Option<String>,
    pub package: Option<String>,
    pub target: Option<String>,
    pub inherited: bool,
    pub extras: Vec<String>,
    pub marker: Option<String>,
}

impl<'tree> Dependency<'tree> {
    fn new(name: String, node: Node<'tree>, group: &str) -> Self {
        Self {
            name,
            node,
            table: None,
            group: group.to_string(),
            version: None,
            package: None,
            target: None,
            inherited: false,
            extras: Vec::new(),
            marker: None,
        }
    }
}

pub(crate) fn collect_dependencies<'tree>(
    manifest: Manifest,
    root: Node<'tree>,
    content: &str,
) -> Vec<Dependency<'tree>> {
    let mut dependencies = Vec::new();
    let mut cursor = root.walk();
    for table in root
        .named_children(&mut cursor)
        .filter(|node| matches!(node.kind(), "table" | "table_array_element"))
    {
        let Some(header) = header_parts(table, content) else {
            continue;
        };
        let header: Vec<&str> = header.iter().map(String::as_str).collect();
        match manifest {
            Manifest::Cargo => collect_cargo_table(table, &header, content, &mut dependencies),
            Manifest::Pyproject => {
                collect_pyproject_table(table, &header, content, &mut dependencies)
            }
            Manifest::Pipfile => match header.as_slice() {
                ["packages"] => {
                    push_poetry_table(table, "pipenv:packages", content, &mut dependencies)
                }
                ["dev-packages"] => {
                    push_poetry_table(table, "pipenv:dev-packages", content, &mut dependencies)
                }
                _ => {}
            },
        }
    }
    dependencies
}

/// Split `target.'cfg(unix)'.dev-dependencies` into `(kind, target)`.
fn cargo_dependency_group<'a>(header: &[&'a str]) -> Option<(&'a str, Option<&'a str>)> {
    let is_kind = |part: &str| {
        matches!(
            part,
            "dependencies" | "dev-dependencies" | "build-dependencies"
        )
    };
    match header {
        [kind] if is_kind(kind) => Some((kind, None)),
        ["workspace", "dependencies"] => Some(("workspace", None)),
        ["target", target, kind] if is_kind(kind) => Some((kind, Some(target))),
        _ => None,
    }
}

fn collect_cargo_table<'tree>(
    table: Node<'tree>,
    header: &[&str],
    content: &str,
    dependencies: &mut Vec<Dependency<'tree>>,
) {
    if let Some((group, target)) = cargo_dependency_group(header) {
        for pair in pairs(table) {
            let Some(key) = pair_key_parts(pair, content) else {
                continue;
            };
            let name = key[0].clone();
            let index = match dependencies
                .iter()
                .position(|dep| dep.table == Some(table) && dep.name == name)
            {
                Some(index) => index,
                None => {
                    let mut dependency = Dependency::new(name, pair, group);
                    dependency.table = Some(table);
                    dependency.target = target.map(str::to_string);
                    dependencies.push(dependency);
                    dependencies.len() - 1
                }
            };
            let dependency = &mut dependencies[index];
            let Some(value) = pair_value(pair) else {
                continue;
            };
            match key.get(1) {
                Some(field) => apply_cargo_field(dependency, field, value, content),
                None if value.kind() == "string" => {
                    dependency.version = string_value(value, content);
                }
                None if value.kind() == "inline_table" => {
                    for field_pair in pairs(value) {
                        if let (Some(field), Some(field_value)) =
                            (pair_key_parts(field_pair, content), pair_value(field_pair))
                        {
                            apply_cargo_field(dependency, &field[0], field_value, content);
                        }
                    }
                }
                None => {}
            }
        }
        return;
    }

    let (name, group_header) = header.split_last().unwrap_or((&"", &[]));
    if let Some((group, target)) = cargo_dependency_group(group_header) {
        let mut dependency = Dependency::new((*name).to_string(), table, group);
        dependency.target = target.map(str::to_string);
        for pair in pairs(table) {
            if let (Some(field), Some(value)) = (pair_key_parts(pair, content), pair_value(pair)) {
                apply_cargo_field(&mut dependency, &field[0], value, content);
            }
        }
        dependencies.push(dependency);
    }
}

fn apply_cargo_field(dependency: &mut Dependency<'_>, field: &str, value: Node<'_>, content: &str) {
    match field {
        "version" => dependency.version = string_value(value, content),
        "package" => dependency.package = string_value(value, content),
        "workspace" => dependency.inherited = node_text(value, content) == Some("true"),
        _ => {}
    }
}

fn collect_pyproject_table<'tree>(
    table: Node<'tree>,
    header: &[&str],
    content: &str,
    dependencies: &mut Vec<Dependency<'tree>>,
) {
    match header {
        ["project"] => {
            for pair in pairs(table) {
                if pair_key_parts(pair, content).as_deref() == Some(&["dependencies".to_string()]) {
                    push_requirements(pair, "runtime", content, dependencies);
                }
            }
        }
        ["project", "optional-dependencies"] | ["dependency-groups"] => {
            let prefix = if header[0] == "project" {
                "optional"
            } else {
                "group"
            };
            for pair in pairs(table) {
                if let Some(key) = pair_key_parts(pair, content) {
                    let group = format!("{prefix}:{}", key.join("."));
                    push_requirements(pair, &group, content, dependencies);
                }
            }
        }
        ["build-system"] => {
            for pair in pairs(table) {
                if pair_key_parts(pair, content).as_deref() == Some(&["requires".to_string()]) {
                    push_requirements(pair, "build-system", content, dependencies);
                }
            }
        }
        ["tool", "poetry", "dependencies"] => {
            push_poetry_table(table, "poetry:main", content, dependencies)
        }
        ["tool", "poetry", "dev-dependencies"] => {
            push_poetry_table(table, "poetry:dev", content, dependencies)
        }
        ["tool", "poetry", "group", group, "dependencies"] => {
            push_poetry_table(table, &format!("poetry:{group}"), content, dependencies)
        }
        _ => {}
    }
}

fn push_poetry_table<'tree>(
    table: Node<'tree>,
    group: &str,
    content: &str,
    dependencies: &mut Vec<Dependency<'tree>>,
) {
    for pair in pairs(table) {
        let Some(key) = pair_key_parts(pair, content) else {
            continue;
        };
        if key.len() != 1 || key[0] == "python" {
            continue;
        }
        let mut dependency = Dependency::new(normalize_python_name(&key[0]), pair, group);
        dependency.table = Some(table);
        match pair_value(pair) {
            Some(value) if value.kind() == "string" => {
                dependency.version = string_value(value, content);
            }
            Some(value) if value.kind() == "inline_table" => {
                for field_pair in pairs(value) {
                    if pair_key_parts(field_pair, content).as_deref()
                        == Some(&["version".to_string()])
                    {
                        dependency.version =
                            pair_value(field_pair).and_then(|v| string_value(v, content));
                    }
                }
            }
            _ => {}
        }
        dependencies.push(dependency);
    }
}

fn push_requirements<'tree>(
    pair: Node<'tree>,
    group: &str,
    content: &str,
    dependencies: &mut Vec<Dependency<'tree>>,
) {
    let Some(array) = pair_value(pair).filter(|value| value.kind() == "array") else {
        return;
    };
    let mut cursor = array.walk();
    for item in array
        .named_children(&mut cursor)
        .filter(|item| item.kind() == "string")
    {
        if let Some(dependency) = string_value(item, content)
            .and_then(|requirement| parse_requirement(&requirement, item, group))
        {
            dependencies.push(dependency);
        }
    }
}

/// Parse a PEP 508 requirement such as `sqlalchemy[asyncio]>=2.0; python_version < "3.13"`.
fn parse_requirement<'tree>(
    requirement: &str,
    node: Node<'tree>,
    group: &str,
) -> Option<Dependency<'tree>> {
    let (spec, marker) = match requirement.split_once(';') {
        Some((spec, marker)) => (spec.trim(), Some(marker.trim())),
        None => (requirement.trim(), None),
    };
    let name_end = spec
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
        .unwrap_or(spec.len());
    if name_end == 0 {
        return None;
    }
    let mut dependency = Dependency::new(normalize_python_name(&spec[..name_end]), node, group);
    let mut rest = spec[name_end..].trim_start();
    if let Some(inner) = rest.strip_prefix('[')
        && let Some((extras, after)) = inner.split_once(']')
    {
        dependency.extras = extras
            .split(',')
            .map(str::trim)
            .filter(|extra| !extra.is_empty())
            .map(str::to_string)
            .collect();
        rest = after.trim_start();
    }
    dependency.version = (!rest.is_empty()).then(|| rest.to_string());
    dependency.marker = marker.filter(|m| !m.is_empty()).map(str::to_string);
    Some(dependency)
}

/// PEP 503 name normalization: lower case, runs of `-`, `_`, `.` become `-`.
fn normalize_python_name(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '-' | '_' | '.') {
            if !normalized.ends_with('-') {
                normalized.push('-');
            }
        } else {
            normalized.push(ch.to_ascii_lowercase());
        }
    }
    normalized
}

pub(crate) fn pairs<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "pair")
        .collect()
}

pub(crate) fn header_parts(table: Node<'_>, content: &str) -> Option<Vec<String>> {
    let mut cursor = table.walk();
    let key = table
        .named_children(&mut cursor)
        .find(|child| is_key(child.kind()))?;
    Some(key_parts(key, content))
}

pub(crate) fn pair_key_parts(pair: Node<'_>, content: &str) -> Option<Vec<String>> {
    let key = pair.named_child(0).filter(|key| is_key(key.kind()))?;
    Some(key_parts(key, content))
}

fn is_key(kind: &str) -> bool {
    matches!(kind, "bare_key" | "quoted_key" | "dotted_key")
}

fn key_parts(key: Node<'_>, content: &str) -> Vec<String> {
    key_parts_at(key, content, 0)
}

fn key_parts_at(key: Node<'_>, content: &str, depth: u32) -> Vec<String> {
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return Vec::new();
    };
    if key.kind() != "dotted_key" {
        return node_text(key, content)
            .map(|text| vec![super::text::decode_toml_key(text)])
            .unwrap_or_default();
    }
    let mut cursor = key.walk();
    key.named_children(&mut cursor)
        .flat_map(|part| key_parts_at(part, content, child_depth))
        .collect()
}

pub(crate) fn string_value(node: Node<'_>, content: &str) -> Option<String> {
    (node.kind() == "string").then_some(())?;
    super::text::decode_toml_string(node_text(node, content)?).map(|(value, _)| value)
}

fn node_text<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

pub(crate) const MANIFEST_DEPENDENCY_PATTERN_ID: &str = "manifest.dependency.v1";

/// One `manifest.dependency.v1` fact per dependency declared in a Cargo or
/// pyproject manifest.
pub(crate) fn dependency_facts(
    root: Node<'_>,
    file_path: &str,
    content: &str,
) -> Vec<crate::base::StructuralFact> {
    use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};
    use serde_json::Value;

    let Some(manifest) = Manifest::for_path(file_path) else {
        return Vec::new();
    };
    collect_dependencies(manifest, root, content)
        .into_iter()
        .map(|dependency| {
            let mut metadata = base_metadata("dependencies");
            insert_string(&mut metadata, "ecosystem", manifest.ecosystem());
            insert_string(&mut metadata, "name", &dependency.name);
            insert_string(&mut metadata, "group", &dependency.group);
            for (key, value) in [
                ("version", &dependency.version),
                ("package", &dependency.package),
                ("target", &dependency.target),
                ("marker", &dependency.marker),
            ] {
                if let Some(value) = value {
                    insert_string(&mut metadata, key, value);
                }
            }
            if dependency.inherited {
                metadata.insert("workspace".to_string(), Value::Bool(true));
            }
            if !dependency.extras.is_empty() {
                metadata.insert(
                    "extras".to_string(),
                    Value::Array(dependency.extras.into_iter().map(Value::String).collect()),
                );
            }
            fact_for_node(
                file_path,
                "toml",
                MANIFEST_DEPENDENCY_PATTERN_ID,
                "dependency",
                dependency.node,
                metadata,
            )
        })
        .collect()
}

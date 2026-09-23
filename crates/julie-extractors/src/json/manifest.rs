//! Package and project manifests written in JSON.
//!
//! - `package.json` (npm) and `composer.json` (Composer): each dependency is an
//!   `Imports` edge from its group key (`dependencies`, `require-dev`, ...) to
//!   the dependency key, plus a `manifest.dependency.v1` fact. Each `scripts`
//!   entry is a `manifest.script.v1` fact.
//! - `tsconfig*.json` and `jsconfig.json`: `extends` and `references[].path`
//!   name other files, so each is a structured pending `References` row whose
//!   import context is the path as written.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Node;

use super::decode_json_string;
use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};
use crate::base::{
    BaseExtractor, NormalizedSpan, Relationship, RelationshipKind, StructuralFact,
    StructuredPendingRelationship, Symbol, UnresolvedTarget,
};

pub(crate) const MANIFEST_SCRIPT_PATTERN_ID: &str = "manifest.script.v1";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Manifest {
    Npm,
    Composer,
    TsConfig,
}

impl Manifest {
    fn for_path(file_path: &str) -> Option<Self> {
        let name = file_path.rsplit(['/', '\\']).next()?;
        match name {
            "package.json" => Some(Self::Npm),
            "composer.json" => Some(Self::Composer),
            "jsconfig.json" => Some(Self::TsConfig),
            _ if name.starts_with("tsconfig") && name.ends_with(".json") => Some(Self::TsConfig),
            _ => None,
        }
    }

    fn ecosystem(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Composer => "composer",
            Self::TsConfig => "",
        }
    }

    fn is_dependency_group(self, key: &str) -> bool {
        match self {
            Self::Npm => matches!(
                key,
                "dependencies" | "devDependencies" | "peerDependencies" | "optionalDependencies"
            ),
            Self::Composer => matches!(key, "require" | "require-dev"),
            Self::TsConfig => false,
        }
    }
}

struct Entry<'tree> {
    key: String,
    pair: Node<'tree>,
    value: Node<'tree>,
}

fn entries<'tree>(object: Node<'tree>, content: &str) -> Vec<Entry<'tree>> {
    if object.kind() != "object" {
        return Vec::new();
    }
    let mut cursor = object.walk();
    object
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "pair")
        .filter_map(|pair| {
            let key = pair.child_by_field_name("key")?;
            Some(Entry {
                key: decode_json_string(content.get(key.start_byte()..key.end_byte())?),
                pair,
                value: pair.child_by_field_name("value")?,
            })
        })
        .collect()
}

fn root_entries<'tree>(root: Node<'tree>, content: &str) -> Vec<Entry<'tree>> {
    let mut cursor = root.walk();
    let object = root
        .named_children(&mut cursor)
        .find(|child| child.kind() == "object");
    object.map_or_else(Vec::new, |object| entries(object, content))
}

fn string_of(node: Node, content: &str) -> Option<String> {
    (node.kind() == "string")
        .then(|| content.get(node.start_byte()..node.end_byte()))
        .flatten()
        .map(decode_json_string)
}

fn symbol_for<'a>(symbols: &'a [Symbol], node: Node) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        symbol.start_byte == node.start_byte() as u32 && symbol.end_byte == node.end_byte() as u32
    })
}

struct Dependency<'tree> {
    group: Entry<'tree>,
    entry: Entry<'tree>,
    version: Option<String>,
}

fn dependencies<'tree>(
    manifest: Manifest,
    root: Node<'tree>,
    content: &str,
) -> Vec<Dependency<'tree>> {
    root_entries(root, content)
        .into_iter()
        .filter(|group| manifest.is_dependency_group(&group.key))
        .flat_map(|group| {
            entries(group.value, content)
                .into_iter()
                .map(|entry| Dependency {
                    version: string_of(entry.value, content),
                    group: Entry {
                        key: group.key.clone(),
                        pair: group.pair,
                        value: group.value,
                    },
                    entry,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn extract_manifest_relationships(
    base: &mut BaseExtractor,
    root: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(manifest) = Manifest::for_path(&base.file_path) else {
        return;
    };
    if manifest == Manifest::TsConfig {
        extract_tsconfig_pending(base, root, symbols);
        return;
    }
    let content = base.content.clone();
    for dependency in dependencies(manifest, root, &content) {
        let (Some(from), Some(to)) = (
            symbol_for(symbols, dependency.group.pair),
            symbol_for(symbols, dependency.entry.pair),
        ) else {
            continue;
        };
        let metadata = HashMap::from([
            (
                "dependencyKind".to_string(),
                Value::String(dependency.group.key.clone()),
            ),
            (
                "dependencyName".to_string(),
                Value::String(dependency.entry.key.clone()),
            ),
        ]);
        let node = dependency.entry.pair;
        relationships.push(Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                from.id,
                to.id,
                RelationshipKind::Imports,
                node.start_position().row
            ),
            from_symbol_id: from.id.clone(),
            to_symbol_id: to.id.clone(),
            kind: RelationshipKind::Imports,
            file_path: base.file_path.clone(),
            line_number: node.start_position().row as u32 + 1,
            span: Some(NormalizedSpan::from_node(&node)),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: Some(metadata),
        });
    }
}

fn extract_tsconfig_pending(base: &mut BaseExtractor, root: Node, symbols: &[Symbol]) {
    let content = base.content.clone();
    let mut targets: Vec<(Node, String)> = Vec::new();
    for entry in root_entries(root, &content) {
        match entry.key.as_str() {
            "extends" => {
                let mut cursor = entry.value.walk();
                let values: Vec<Node> = match entry.value.kind() {
                    "array" => entry.value.named_children(&mut cursor).collect(),
                    _ => vec![entry.value],
                };
                targets.extend(
                    values
                        .into_iter()
                        .filter_map(|value| string_of(value, &content))
                        .map(|path| (entry.pair, path)),
                );
            }
            "references" if entry.value.kind() == "array" => {
                let mut cursor = entry.value.walk();
                for element in entry.value.named_children(&mut cursor) {
                    for field in entries(element, &content) {
                        if field.key == "path"
                            && let Some(path) = string_of(field.value, &content)
                        {
                            targets.push((field.pair, path));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for (pair, path) in targets {
        let Some(from) = symbol_for(symbols, pair) else {
            continue;
        };
        let terminal_name = super::relationships::document_stem(&path);
        if path.trim().is_empty() || terminal_name.is_empty() {
            continue;
        }
        let target = UnresolvedTarget {
            display_name: path.clone(),
            terminal_name,
            receiver: None,
            namespace_path: Vec::new(),
            import_context: Some(path),
        };
        let pending = StructuredPendingRelationship::new(
            from.id.clone(),
            target,
            Some(from.id.clone()),
            RelationshipKind::References,
            base.file_path.clone(),
            pair.start_position().row as u32 + 1,
            1.0,
        );
        base.add_structured_pending_relationship(pending);
    }
}

/// `manifest.dependency.v1` and `manifest.script.v1` facts for an npm or
/// Composer manifest.
pub(crate) fn manifest_facts(root: Node, file_path: &str, content: &str) -> Vec<StructuralFact> {
    let Some(manifest) = Manifest::for_path(file_path).filter(|m| *m != Manifest::TsConfig) else {
        return Vec::new();
    };
    let mut facts: Vec<StructuralFact> = dependencies(manifest, root, content)
        .into_iter()
        .map(|dependency| {
            let mut metadata = base_metadata("dependencies");
            insert_string(&mut metadata, "ecosystem", manifest.ecosystem());
            insert_string(&mut metadata, "name", &dependency.entry.key);
            insert_string(&mut metadata, "group", &dependency.group.key);
            if let Some(version) = &dependency.version {
                insert_string(&mut metadata, "version", version);
                if version.starts_with("workspace:") {
                    metadata.insert("workspace".to_string(), Value::Bool(true));
                }
            }
            fact_for_node(
                file_path,
                "json",
                crate::toml::dependencies::MANIFEST_DEPENDENCY_PATTERN_ID,
                "dependency",
                dependency.entry.pair,
                metadata,
            )
        })
        .collect();
    let scripts = root_entries(root, content)
        .into_iter()
        .filter(|entry| entry.key == "scripts")
        .flat_map(|entry| entries(entry.value, content));
    for script in scripts {
        let Some(command) = script_command(script.value, content) else {
            continue;
        };
        let mut metadata = base_metadata("pipeline");
        insert_string(&mut metadata, "ecosystem", manifest.ecosystem());
        insert_string(&mut metadata, "name", &script.key);
        insert_string(&mut metadata, "command", &command);
        facts.push(fact_for_node(
            file_path,
            "json",
            MANIFEST_SCRIPT_PATTERN_ID,
            "script",
            script.pair,
            metadata,
        ));
    }
    facts
}

/// A script command; a Composer command list runs its steps in order.
fn script_command(value: Node, content: &str) -> Option<String> {
    if value.kind() == "array" {
        let mut cursor = value.walk();
        let steps: Vec<String> = value
            .named_children(&mut cursor)
            .filter_map(|step| string_of(step, content))
            .collect();
        return (!steps.is_empty()).then(|| steps.join(" && "));
    }
    string_of(value, content).filter(|command| !command.is_empty())
}

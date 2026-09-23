//! Erlang term files: `*.app.src` application resources, `rebar.config`, and
//! `sys.config`. The grammar parses them as a list of dot-terminated terms, so
//! they carry no forms; their semantics live in the tuples.
//!
//! - An application resource `{application, Name, Props}` becomes a Module
//!   symbol. Its `mod` callback module is a pending reference, its `env` keys
//!   are Property symbols, and `applications` entries are OTP dependencies.
//! - `rebar.config` top-level keys are Property symbols, and `deps` (also
//!   inside `profiles`) and `plugins` entries are Hex dependencies.
//! - `sys.config` `[{App, [{Key, Value}]}]` becomes a Module symbol per
//!   application with its keys as nested Property symbols.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::ErlangExtractor;
use super::helpers::{named_children, unquote_atom};
use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};
use crate::base::{
    RelationshipKind, StructuralFact, Symbol, SymbolKind, SymbolOptions, UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const MANIFEST_DEPENDENCY_PATTERN_ID: &str =
    crate::toml::dependencies::MANIFEST_DEPENDENCY_PATTERN_ID;
const MOD_CALLBACK_CONFIDENCE: f32 = 0.9;
const SIGNATURE_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TermConfig {
    AppResource,
    Rebar,
    SysConfig,
}

impl TermConfig {
    pub(crate) fn for_path(file_path: &str) -> Option<Self> {
        let name = file_path.rsplit(['/', '\\']).next()?;
        match name {
            "rebar.config" => Some(Self::Rebar),
            "sys.config" => Some(Self::SysConfig),
            _ if name.len() > ".app.src".len() && name.ends_with(".app.src") => {
                Some(Self::AppResource)
            }
            _ => None,
        }
    }
}

/// The terms of the file, each a `source_file` child.
fn terms<'tree>(tree: &'tree Tree) -> Vec<Node<'tree>> {
    let root = tree.root_node();
    named_children(&root)
        .into_iter()
        .filter(|node| node.kind() != "comment")
        .collect()
}

/// `(key, value)` of a two-element tuple whose key is an atom.
fn pair<'tree>(content: &str, node: Node<'tree>) -> Option<(String, Node<'tree>)> {
    if node.kind() != "tuple" {
        return None;
    }
    let [key, value] = named_children(&node).try_into().ok()?;
    atom(content, key).map(|key| (key, value))
}

fn atom(content: &str, node: Node) -> Option<String> {
    (node.kind() == "atom")
        .then(|| content.get(node.byte_range()))
        .flatten()
        .map(unquote_atom)
}

fn string(content: &str, node: Node) -> Option<String> {
    let text = content.get(node.byte_range())?;
    (node.kind() == "string")
        .then(|| {
            text.strip_prefix('"')?
                .strip_suffix('"')
                .map(str::to_string)
        })
        .flatten()
}

/// The application resource tuple `{application, Name, Props}`.
fn application<'tree>(content: &str, term: Node<'tree>) -> Option<(String, Node<'tree>)> {
    let [tag, name, props] = named_children(&term).try_into().ok()?;
    (term.kind() == "tuple" && atom(content, tag)? == "application" && props.kind() == "list")
        .then_some(())?;
    Some((atom(content, name)?, props))
}

pub(super) fn extract_symbols(
    extractor: &mut ErlangExtractor,
    tree: &Tree,
    config: TermConfig,
) -> Vec<Symbol> {
    let content = extractor.base.content.clone();
    let mut symbols = Vec::new();
    for term in terms(tree) {
        match config {
            TermConfig::AppResource => {
                let Some((name, props)) = application(&content, term) else {
                    continue;
                };
                let app = application_symbol(extractor, term, name);
                let app_id = app.id.clone();
                symbols.push(app);
                for (key, value) in named_children(&props)
                    .into_iter()
                    .filter_map(|prop| pair(&content, prop))
                {
                    if key == "env" {
                        emit_proplist(extractor, &content, value, &app_id, &mut symbols, 0);
                    }
                }
            }
            TermConfig::Rebar => {
                if let Some((key, value)) = pair(&content, term) {
                    let symbol = property(extractor, term, key, value, None);
                    symbols.push(symbol);
                }
            }
            TermConfig::SysConfig => {
                if term.kind() != "list" {
                    continue;
                }
                for entry in named_children(&term) {
                    let Some((name, value)) = pair(&content, entry) else {
                        continue;
                    };
                    let app = application_symbol(extractor, entry, name);
                    let app_id = app.id.clone();
                    symbols.push(app);
                    emit_proplist(extractor, &content, value, &app_id, &mut symbols, 0);
                }
            }
        }
    }
    symbols
}

fn application_symbol(extractor: &mut ErlangExtractor, node: Node, name: String) -> Symbol {
    let options = SymbolOptions {
        signature: Some(signature(extractor, node)),
        metadata: Some(HashMap::from([(
            "erlang_application".to_string(),
            Value::Bool(true),
        )])),
        ..Default::default()
    };
    extractor
        .base
        .create_symbol(&node, name, SymbolKind::Module, options)
}

fn property(
    extractor: &mut ErlangExtractor,
    node: Node,
    key: String,
    value: Node,
    parent_id: Option<&str>,
) -> Symbol {
    let options = SymbolOptions {
        signature: Some(signature(extractor, node)),
        parent_id: parent_id.map(str::to_string),
        ..Default::default()
    };
    let mut symbol = extractor
        .base
        .create_symbol(&node, key, SymbolKind::Property, options);
    extractor.base.set_body_span(
        &mut symbol,
        Some(crate::base::span::NormalizedSpan::from_node(&value)),
    );
    symbol
}

fn signature(extractor: &ErlangExtractor, node: Node) -> String {
    let text = extractor.base.get_node_text(&node);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= SIGNATURE_LIMIT {
        text
    } else {
        let truncated: String = text.chars().take(SIGNATURE_LIMIT).collect();
        format!("{truncated}...")
    }
}

/// Each `{Key, Value}` of a property list becomes a Property symbol; a value
/// that is itself a property list nests its keys under the key.
fn emit_proplist(
    extractor: &mut ErlangExtractor,
    content: &str,
    list: Node,
    parent_id: &str,
    symbols: &mut Vec<Symbol>,
    depth: u32,
) {
    if list.kind() != "list" || !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for entry in named_children(&list) {
        let Some((key, value)) = pair(content, entry) else {
            continue;
        };
        let symbol = property(extractor, entry, key, value, Some(parent_id));
        let id = symbol.id.clone();
        symbols.push(symbol);
        emit_proplist(extractor, content, value, &id, symbols, child_depth);
    }
}

/// The `{mod, {Module, Args}}` callback of an application resource is a
/// pending reference from the application to its callback module.
pub(super) fn extract_relationships(
    extractor: &mut ErlangExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) {
    let content = extractor.base.content.clone();
    for term in terms(tree) {
        let Some((_, props)) = application(&content, term) else {
            continue;
        };
        let Some(app) = symbols.iter().find(|symbol| {
            symbol.kind == SymbolKind::Module && symbol.start_byte == term.start_byte() as u32
        }) else {
            continue;
        };
        let callback = named_children(&props)
            .into_iter()
            .filter_map(|prop| pair(&content, prop))
            .find(|(key, value)| key == "mod" && value.kind() == "tuple")
            .and_then(|(_, value)| named_children(&value).first().copied())
            .and_then(|module| atom(&content, module).map(|name| (module, name)));
        let Some((module, name)) = callback else {
            continue;
        };
        let pending = extractor.base.create_pending_relationship_at_target(
            app.id.clone(),
            UnresolvedTarget::simple(name),
            RelationshipKind::References,
            &module,
            Some(app.id.clone()),
            Some(MOD_CALLBACK_CONFIDENCE),
        );
        extractor.base.add_structured_pending_relationship(pending);
    }
}

/// `manifest.dependency.v1` facts for application resource `applications`
/// (ecosystem `otp`) and `rebar.config` `deps`/`plugins` (ecosystem `hex`).
pub(crate) fn dependency_facts(tree: &Tree, file_path: &str, content: &str) -> Vec<StructuralFact> {
    let Some(config) = TermConfig::for_path(file_path) else {
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    for term in terms(tree) {
        match config {
            TermConfig::AppResource => {
                let Some((_, props)) = application(content, term) else {
                    continue;
                };
                for (key, value) in named_children(&props)
                    .into_iter()
                    .filter_map(|prop| pair(content, prop))
                {
                    if matches!(
                        key.as_str(),
                        "applications" | "included_applications" | "optional_applications"
                    ) {
                        collect_dependencies(content, value, "otp", &key, &mut dependencies);
                    }
                }
            }
            TermConfig::Rebar => {
                let Some((key, value)) = pair(content, term) else {
                    continue;
                };
                match key.as_str() {
                    "deps" | "plugins" | "project_plugins" => {
                        collect_dependencies(content, value, "hex", &key, &mut dependencies)
                    }
                    "profiles" => collect_profile_dependencies(content, value, &mut dependencies),
                    _ => {}
                }
            }
            TermConfig::SysConfig => {}
        }
    }
    dependencies
        .into_iter()
        .map(|dependency| {
            let mut metadata = base_metadata("dependencies");
            insert_string(&mut metadata, "ecosystem", dependency.ecosystem);
            insert_string(&mut metadata, "name", &dependency.name);
            insert_string(&mut metadata, "group", &dependency.group);
            if let Some(version) = &dependency.version {
                insert_string(&mut metadata, "version", version);
            }
            fact_for_node(
                file_path,
                "erlang",
                MANIFEST_DEPENDENCY_PATTERN_ID,
                "dependency",
                dependency.node,
                metadata,
            )
        })
        .collect()
}

struct Dependency<'tree> {
    node: Node<'tree>,
    ecosystem: &'static str,
    name: String,
    group: String,
    version: Option<String>,
}

fn collect_profile_dependencies<'tree>(
    content: &str,
    profiles: Node<'tree>,
    dependencies: &mut Vec<Dependency<'tree>>,
) {
    if profiles.kind() != "list" {
        return;
    }
    for (profile, settings) in named_children(&profiles)
        .into_iter()
        .filter_map(|entry| pair(content, entry))
    {
        if settings.kind() != "list" {
            continue;
        }
        for (key, value) in named_children(&settings)
            .into_iter()
            .filter_map(|entry| pair(content, entry))
        {
            if key == "deps" {
                let group = format!("profile:{profile}");
                collect_dependencies(content, value, "hex", &group, dependencies);
            }
        }
    }
}

/// Entries are `Name`, `{Name, Vsn}`, `{Name, Source}`, or the rebar2
/// `{Name, Vsn, Source}`; only a string `Vsn` is a version.
fn collect_dependencies<'tree>(
    content: &str,
    list: Node<'tree>,
    ecosystem: &'static str,
    group: &str,
    dependencies: &mut Vec<Dependency<'tree>>,
) {
    if list.kind() != "list" {
        return;
    }
    for entry in named_children(&list) {
        let (name, version) = if entry.kind() == "tuple" {
            let elements = named_children(&entry);
            let Some(name) = elements.first().and_then(|name| atom(content, *name)) else {
                continue;
            };
            (name, elements.get(1).and_then(|vsn| string(content, *vsn)))
        } else if let Some(name) = atom(content, entry) {
            (name, None)
        } else {
            continue;
        };
        dependencies.push(Dependency {
            node: entry,
            ecosystem,
            name,
            group: group.to_string(),
            version,
        });
    }
}

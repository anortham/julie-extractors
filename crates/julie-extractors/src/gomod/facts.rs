//! Structural facts for `go.mod`: one fact per directive line. A `require`
//! line is a shared `manifest.dependency.v1` fact with ecosystem `go`; every
//! other directive has its own `gomod.<directive>.v1` pattern.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Tree;

use super::directives::{self, Directive, Entry, entries};
use crate::base::StructuralFact;
use crate::base::structural_fact_builders::{base_metadata, fact_for_span, insert_string};
use crate::toml::dependencies::MANIFEST_DEPENDENCY_PATTERN_ID;

pub(crate) const PATTERN_IDS: &[&str] = &[
    MANIFEST_DEPENDENCY_PATTERN_ID,
    "gomod.exclude.v1",
    "gomod.go.v1",
    "gomod.godebug.v1",
    "gomod.ignore.v1",
    "gomod.module.v1",
    "gomod.replace.v1",
    "gomod.retract.v1",
    "gomod.tool.v1",
    "gomod.toolchain.v1",
];

pub(crate) fn structural_facts(tree: &Tree, file_path: &str, content: &str) -> Vec<StructuralFact> {
    let mut block_rationales = HashMap::new();
    entries(tree.root_node(), content)
        .iter()
        .filter_map(|entry| fact(entry, file_path, content, &mut block_rationales))
        .collect()
}

fn fact(
    entry: &Entry<'_>,
    file_path: &str,
    content: &str,
    block_rationales: &mut HashMap<usize, (String, bool)>,
) -> Option<StructuralFact> {
    let mut metadata = base_metadata("dependencies");
    let copy = |metadata: &mut HashMap<String, Value>, roles: &[&str]| {
        for role in roles {
            if let Some(value) = entry.value(role) {
                insert_string(metadata, role, value);
            }
        }
    };
    let (pattern_id, capture_name) = match entry.directive {
        Directive::Require => {
            insert_string(&mut metadata, "ecosystem", "go");
            insert_string(&mut metadata, "name", entry.value("module_path")?);
            insert_string(&mut metadata, "group", "require");
            copy(&mut metadata, &["version"]);
            metadata.insert(
                "indirect".to_string(),
                Value::Bool(directives::is_indirect(content, entry)),
            );
            (MANIFEST_DEPENDENCY_PATTERN_ID, "dependency")
        }
        Directive::Module => {
            copy(&mut metadata, &["module_path"]);
            if let Some(deprecated) = directives::deprecation(content, entry) {
                insert_string(&mut metadata, "deprecated", &deprecated);
            }
            ("gomod.module.v1", "module")
        }
        Directive::Go => {
            copy(&mut metadata, &["version"]);
            ("gomod.go.v1", "go")
        }
        Directive::Toolchain => {
            copy(&mut metadata, &["toolchain"]);
            ("gomod.toolchain.v1", "toolchain")
        }
        Directive::Exclude => {
            copy(&mut metadata, &["module_path", "version"]);
            ("gomod.exclude.v1", "exclude")
        }
        Directive::Replace => {
            copy(
                &mut metadata,
                &[
                    "module_path",
                    "version",
                    "replacement",
                    "replacement_version",
                ],
            );
            metadata.insert(
                "local".to_string(),
                Value::Bool(entry.value("replacement_version").is_none()),
            );
            ("gomod.replace.v1", "replace")
        }
        Directive::Retract => {
            let low = entry.value("low").or(entry.value("version"))?;
            let high = entry.value("high").or(entry.value("version"))?;
            insert_string(&mut metadata, "low", low);
            insert_string(&mut metadata, "high", high);
            metadata.insert("range".to_string(), Value::Bool(entry.range));
            let (rationale, truncated) =
                directives::retract_rationale(content, entry, block_rationales);
            if !rationale.is_empty() {
                insert_string(&mut metadata, "rationale", &rationale);
            }
            if truncated {
                metadata.insert("rationale_truncated".to_string(), Value::Bool(true));
            }
            ("gomod.retract.v1", "retract")
        }
        Directive::Tool => {
            copy(&mut metadata, &["package_path"]);
            ("gomod.tool.v1", "tool")
        }
        Directive::Ignore => {
            copy(&mut metadata, &["path"]);
            ("gomod.ignore.v1", "ignore")
        }
        Directive::Godebug => {
            copy(&mut metadata, &["key", "value"]);
            ("gomod.godebug.v1", "godebug")
        }
    };
    let span =
        crate::base::NormalizedSpan::from_content_range(content, entry.start_byte, entry.end_byte)?;
    Some(fact_for_span(
        file_path,
        "gomod",
        pattern_id,
        capture_name,
        entry.node.kind(),
        span,
        metadata,
    ))
}

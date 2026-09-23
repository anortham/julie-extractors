//! PowerShell module imports, exports, and dot-sourcing
//! Handles Import-Module, Export-ModuleMember, using statements, and dot sourcing

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

use super::helpers::{argument_words, command_arguments, module_stem};

static USING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"using\s+(?:namespace|module)\s+([A-Za-z0-9.-_]+)").unwrap());

static REQUIRES_MODULES_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^#requires\s+-modules?\s+(.+)$").unwrap());

static MODULE_NAME_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)ModuleName\s*=\s*["']?([^"';}\s]+)"#).unwrap());

/// One import, export, or alias row: its name, kind, and metadata.
type ModuleRow = (String, SymbolKind, Option<HashMap<String, Value>>);

/// The commands [`extract_import_command`] models as import, export, or alias
/// rows. PowerShell command names ignore case.
pub(super) fn is_module_command(name: &str) -> bool {
    [
        "Import-Module",
        "Import-DscResource",
        "Export-ModuleMember",
        "Set-Alias",
        "New-Alias",
        "using",
    ]
    .iter()
    .any(|command| command.eq_ignore_ascii_case(name))
}

/// Import, export, and alias rows of a module command: one row per module
/// imported, member exported, or alias defined.
pub(super) fn extract_import_command(
    base: &mut BaseExtractor,
    node: Node,
    command_name: &str,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let signature = base.get_node_text(&node).trim().to_string();
    let command = command_name.to_ascii_lowercase();
    let arguments = command_arguments(base, node);
    let words_bound_to = |base: &BaseExtractor, parameter: Option<&str>| -> Vec<String> {
        arguments
            .iter()
            .filter(|(bound, _)| bound.as_deref() == parameter)
            .flat_map(|(_, value)| argument_words(base, *value))
            .collect()
    };

    let rows: Vec<ModuleRow> = match command.as_str() {
        "import-module" | "import-dscresource" => {
            let name_parameter = if command == "import-module" {
                "name"
            } else {
                "modulename"
            };
            let mut words = words_bound_to(base, Some(name_parameter));
            words.extend(words_bound_to(base, None));
            words
                .iter()
                .filter_map(|word| module_stem(word))
                .map(|name| (name, SymbolKind::Import, None))
                .collect()
        }
        "export-modulemember" => ["function", "cmdlet", "variable", "alias"]
            .iter()
            .flat_map(|kind| {
                let mut words = words_bound_to(base, Some(kind));
                if *kind == "function" {
                    words.extend(words_bound_to(base, None));
                }
                words.into_iter().map(move |word| {
                    let metadata = HashMap::from([(
                        "export_kind".to_string(),
                        Value::String(kind.to_string()),
                    )]);
                    (word, SymbolKind::Export, Some(metadata))
                })
            })
            .collect(),
        "set-alias" | "new-alias" => {
            let positional = words_bound_to(base, None);
            let mut positional = positional.into_iter();
            let alias = words_bound_to(base, Some("name"))
                .into_iter()
                .next()
                .or_else(|| positional.next());
            let target = words_bound_to(base, Some("value"))
                .into_iter()
                .next()
                .or_else(|| positional.next());
            match (alias, target) {
                (Some(alias), Some(target)) => {
                    let metadata = HashMap::from([
                        ("aliasTarget".to_string(), Value::String(target)),
                        (
                            "command".to_string(),
                            Value::String(command_name.to_string()),
                        ),
                    ]);
                    vec![(alias, SymbolKind::Import, Some(metadata))]
                }
                _ => Vec::new(),
            }
        }
        _ => USING_RE
            .captures(&signature)
            .and_then(|captures| captures.get(1))
            .map(|name| vec![(name.as_str().to_string(), SymbolKind::Import, None)])
            .unwrap_or_default(),
    };

    let doc_comment = super::documentation::extract_powershell_doc_comment(base, &node);
    rows.into_iter()
        .filter(|(name, _, _)| !name.is_empty())
        .map(|(name, kind, metadata)| {
            base.create_symbol(
                &node,
                name,
                kind,
                SymbolOptions {
                    signature: Some(signature.clone()),
                    visibility: Some(Visibility::Public),
                    parent_id: parent_id.map(|s| s.to_string()),
                    metadata,
                    doc_comment: doc_comment.clone(),
                    annotations: Vec::new(),
                },
            )
        })
        .collect()
}

/// Import rows for the modules a `#Requires -Modules` directive names, in
/// both the bare (`Az.Accounts`) and hashtable (`@{ ModuleName = 'X' }`)
/// forms.
pub(super) fn extract_requires_modules(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let text = base.get_node_text(&node);
    let Some(list) = REQUIRES_MODULES_RE
        .captures(&text)
        .and_then(|captures| captures.get(1))
    else {
        return Vec::new();
    };
    let list = list.as_str();
    let names: Vec<String> = if list.contains("@{") {
        MODULE_NAME_KEY_RE
            .captures_iter(list)
            .filter_map(|captures| captures.get(1))
            .map(|name| name.as_str().to_string())
            .chain(
                strip_hashtables(list)
                    .split(',')
                    .map(|name| name.trim().trim_matches(['"', '\'']).to_string()),
            )
            .collect()
    } else {
        list.split(',')
            .map(|name| name.trim().trim_matches(['"', '\'']).to_string())
            .collect()
    };
    let signature = text.trim().to_string();
    names
        .into_iter()
        .filter(|name| !name.is_empty())
        .map(|name| {
            base.create_symbol(
                &node,
                name,
                SymbolKind::Import,
                SymbolOptions {
                    signature: Some(signature.clone()),
                    visibility: Some(Visibility::Public),
                    parent_id: parent_id.map(|s| s.to_string()),
                    metadata: None,
                    doc_comment: None,
                    annotations: Vec::new(),
                },
            )
        })
        .collect()
}

fn strip_hashtables(list: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    let mut chars = list.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '@' if chars.peek() == Some(&'{') => {
                chars.next();
                depth += 1;
            }
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Extract dot sourcing (e.g., '. "$PSScriptRoot\CommonFunctions.ps1"').
/// A dynamic path (`. $f.FullName`) names no script.
pub(super) fn extract_dot_sourcing(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let command_name_expr_node = node
        .children(&mut cursor)
        .find(|child| child.kind() == "command_name_expr")?;

    let script_path = argument_words(base, command_name_expr_node).pop()?;
    let file_name = module_stem(&script_path)?;
    let signature = base.get_node_text(&node).trim().to_string();

    let mut symbol = base.create_symbol(
        &node,
        file_name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: None,
            doc_comment: super::documentation::extract_powershell_doc_comment(base, &node),
            annotations: Vec::new(),
        },
    );
    symbol.body_span = None;
    symbol.body_hash = None;
    Some(symbol)
}

/// A script module (`.psm1`) that calls `Export-ModuleMember` exports only the
/// functions it names, so every other top-level function is private.
pub(super) fn narrow_to_exported_functions(file_path: &str, symbols: &mut [Symbol]) {
    if !file_path.to_ascii_lowercase().ends_with(".psm1") {
        return;
    }
    let exports: Vec<&Symbol> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Export)
        .collect();
    if exports.is_empty() {
        return;
    }
    let function_patterns: Vec<String> = exports
        .iter()
        .filter(|symbol| {
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("export_kind"))
                .and_then(Value::as_str)
                == Some("function")
        })
        .map(|symbol| symbol.name.to_ascii_lowercase())
        .collect();
    for symbol in symbols.iter_mut().filter(|symbol| {
        symbol.kind == SymbolKind::Function
            && symbol.parent_id.is_none()
            && symbol.metadata.as_ref().is_none_or(|metadata| {
                ["role", "is_test", "test_container"]
                    .iter()
                    .all(|key| !metadata.contains_key(*key))
            })
    }) {
        let name = symbol.name.to_ascii_lowercase();
        if !function_patterns
            .iter()
            .any(|pattern| wildcard_match(pattern, &name))
        {
            symbol.visibility = Some(Visibility::Private);
        }
    }
}

/// PowerShell `*` wildcard matching, as `Export-ModuleMember` applies it.
fn wildcard_match(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(index) => rest = &rest[index + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

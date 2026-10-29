//! Shared test utilities for julie-extractors tests
//!
//! Common helpers used across language extractor test suites.

use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, StructuralFact, Symbol, SymbolKind};
use crate::language::get_tree_sitter_language;

/// Initialize a tree-sitter parser for the given language and parse the code.
///
/// # Arguments
/// * `code` - Source code to parse
/// * `language` - Language identifier (e.g., "go", "csharp", "python")
///
/// # Returns
/// Parsed tree-sitter Tree
///
/// # Panics
/// Panics if the language is not supported or parsing fails.
pub fn init_parser(code: &str, language: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    let ts_language = get_tree_sitter_language(language)
        .unwrap_or_else(|_| panic!("Unsupported language: {}", language));
    parser
        .set_language(&ts_language)
        .unwrap_or_else(|e| panic!("Failed to set language '{}': {}", language, e));
    parser
        .parse(code, None)
        .unwrap_or_else(|| panic!("Failed to parse {} code", language))
}

/// Retrieve a string value from a structural fact's metadata map by key.
pub fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

/// Each import and export symbol as its name and its metadata as a JSON
/// object, in extraction order.
pub fn import_rows_with_metadata(results: &ExtractionResults) -> Vec<(String, serde_json::Value)> {
    results
        .symbols
        .iter()
        .filter(|symbol| {
            matches!(
                symbol.kind,
                crate::base::SymbolKind::Import | crate::base::SymbolKind::Export
            )
        })
        .map(|symbol| {
            let metadata = symbol.metadata.clone().unwrap_or_default();
            (
                symbol.name.clone(),
                serde_json::Value::Object(metadata.into_iter().collect()),
            )
        })
        .collect()
}

/// Filter extraction results for structural facts matching a given pattern ID.
pub fn facts_with_pattern<'a>(
    results: &'a ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

fn symbol_name_by_id(results: &ExtractionResults, id: &str) -> String {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_else(|| "-".to_string())
}

/// Each symbol as `kind name < parent` in source order, leaving out
/// variables (parameters among them), imports, and export rows.
pub fn owner_rows(results: &ExtractionResults) -> Vec<String> {
    let mut symbols: Vec<&Symbol> = results
        .symbols
        .iter()
        .filter(|symbol| {
            !matches!(
                symbol.kind,
                SymbolKind::Variable | SymbolKind::Import | SymbolKind::Export
            )
        })
        .collect();
    symbols.sort_by_key(|symbol| (symbol.start_byte, std::cmp::Reverse(symbol.end_byte)));
    symbols
        .into_iter()
        .map(|symbol| {
            let parent = symbol
                .parent_id
                .as_deref()
                .map(|id| symbol_name_by_id(results, id))
                .unwrap_or_else(|| "-".to_string());
            format!("{:?} {} < {}", symbol.kind, symbol.name, parent).to_lowercase()
        })
        .collect()
}

/// The symbol with this name, panicking with every row when it is missing.
pub fn symbol_named<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("no symbol {name}: {:?}", owner_rows(results)))
}

/// A string value from a symbol's metadata.
pub fn symbol_metadata<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

/// The supertype edges of a file, sorted: `kind from -> to` for a resolved
/// relationship and `kind from -> ?name` for a pending one.
pub fn supertype_edges(results: &ExtractionResults) -> Vec<String> {
    let is_supertype = |kind: &RelationshipKind| {
        matches!(
            kind,
            RelationshipKind::Extends | RelationshipKind::Implements
        )
    };
    let resolved = results
        .relationships
        .iter()
        .filter(|relationship| is_supertype(&relationship.kind))
        .map(|relationship| {
            format!(
                "{:?} {} -> {}",
                relationship.kind,
                symbol_name_by_id(results, &relationship.from_symbol_id),
                symbol_name_by_id(results, &relationship.to_symbol_id)
            )
        });
    let pending = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| is_supertype(&pending.pending.kind))
        .map(|pending| {
            format!(
                "{:?} {} -> ?{}",
                pending.pending.kind,
                symbol_name_by_id(results, &pending.pending.from_symbol_id),
                pending.target.terminal_name
            )
        });
    let mut edges: Vec<String> = resolved
        .chain(pending)
        .map(|edge| edge.to_lowercase())
        .collect();
    edges.sort();
    edges
}

/// Each call identifier in source order as `callee in containing symbol`.
pub fn call_owners(results: &ExtractionResults) -> Vec<String> {
    let mut calls: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call)
        .collect();
    calls.sort_by_key(|identifier| identifier.start_byte);
    calls
        .into_iter()
        .map(|identifier| {
            let owner = identifier
                .containing_symbol_id
                .as_deref()
                .map(|id| symbol_name_by_id(results, id))
                .unwrap_or_else(|| "-".to_string());
            format!("{} in {}", identifier.name, owner)
        })
        .collect()
}

/// Each symbol with a test flag as `name: flags`, sorted, where the flags are
/// the `is_test`, `test_container`, `test_lifecycle`, and `test_role` keys it has.
pub fn test_flags(results: &ExtractionResults) -> Vec<String> {
    let mut rows: Vec<String> = results
        .symbols
        .iter()
        .filter_map(|symbol| {
            let metadata = symbol.metadata.as_ref()?;
            let flags: Vec<&str> = ["is_test", "test_container", "test_lifecycle", "test_role"]
                .into_iter()
                .filter(|key| metadata.contains_key(*key))
                .collect();
            (!flags.is_empty()).then(|| format!("{}: {}", symbol.name, flags.join(",")))
        })
        .collect();
    rows.sort();
    rows
}

/// The name and signature of each test and test container that extraction of
/// `code` at `path` writes, in source order.
pub fn test_names(language: &str, path: &str, code: &str) -> Vec<(String, String)> {
    let mut symbols: Vec<Symbol> = crate::extract_canonical_for_language_at(
        language,
        path,
        code,
        std::path::Path::new("."),
        crate::ExtractionLevel::Facts,
    )
    .expect("extraction")
    .symbols
    .into_iter()
    .filter(|symbol| {
        symbol.metadata.as_ref().is_some_and(|metadata| {
            metadata.contains_key("is_test") || metadata.contains_key("test_container")
        })
    })
    .collect();
    symbols.sort_by_key(|symbol| symbol.start_byte);
    symbols
        .into_iter()
        .map(|symbol| (symbol.name, symbol.signature.unwrap_or_default()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::init_parser;

    #[test]
    fn php_parser_uses_language_php() {
        let tree = init_parser("<?php function load(): void {}", "php");
        assert_eq!(tree.root_node().kind(), "program");
        assert!(!tree.root_node().has_error());
    }
}

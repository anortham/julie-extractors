//! Shared test utilities for julie-extractors tests
//!
//! Common helpers used across language extractor test suites.

use crate::ExtractionResults;
use crate::base::StructuralFact;
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

//! Qt's QML JavaScript directives (`.pragma`, `.import`) at the head of a `.js` file.

use crate::base::{ExtractionLevel, ExtractionResults, StructuralFact, Symbol, SymbolKind};
use crate::javascript::qml_directives::blank_directives;
use crate::pipeline::extract_canonical_at;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical_at(
        "shell/Commons/BorderGeometry.js",
        source,
        Path::new("/repo"),
        ExtractionLevel::Facts,
    )
    .expect("Qt JavaScript with directives should extract")
}

fn imports(results: &ExtractionResults) -> Vec<&Symbol> {
    results
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .collect()
}

fn meta(symbol: &Symbol, key: &str) -> String {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
        .unwrap_or_else(|| panic!("import symbol is missing metadata key {key}"))
        .to_string()
}

fn directive_facts(results: &ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "javascript.qml_directive.v1")
        .collect()
}

fn fact_meta(fact: &StructuralFact, key: &str) -> String {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
        .unwrap_or_else(|| panic!("directive fact is missing metadata key {key}"))
        .to_string()
}

#[test]
fn directive_lines_are_blanked_to_the_same_byte_length() {
    let source = ".pragma library\n.import QtQuick 2.0 as QQ\nvar x = 1\n";

    let blanked = blank_directives(source).expect("directive lines should be blanked");

    assert_eq!(blanked.len(), source.len());
    assert_eq!(
        blanked,
        format!("{}\n{}\nvar x = 1\n", " ".repeat(15), " ".repeat(25))
    );
}

#[test]
fn crlf_directive_lines_keep_their_terminator() {
    let source = ".pragma library\r\nvar x = 1\r\n";

    let blanked = blank_directives(source).expect("directive lines should be blanked");

    assert_eq!(blanked, "               \r\nvar x = 1\r\n");
}

#[test]
fn utf8_text_after_a_directive_keeps_its_bytes() {
    let source = ".pragma library\nvar greeting = \"héllo → ok\"\n";

    let blanked = blank_directives(source).expect("directive lines should be blanked");

    assert_eq!(blanked.len(), source.len());
    assert!(blanked.ends_with("var greeting = \"héllo → ok\"\n"));
}

#[test]
fn a_directive_after_comments_and_blank_lines_is_recognized() {
    let source = "// SPDX-License-Identifier: GPL-3.0\n/* block\n   comment */\n\n.pragma library\nvar x = 1\n";

    let blanked = blank_directives(source).expect("directive lines should be blanked");

    assert!(blanked.starts_with("// SPDX-License-Identifier: GPL-3.0\n"));
    assert!(!blanked.contains(".pragma"));
    assert_eq!(blanked.len(), source.len());
}

#[test]
fn a_line_that_starts_with_a_dot_but_is_not_a_directive_stays() {
    assert_eq!(blank_directives(".5\n"), None);
    assert_eq!(blank_directives(".importantly()\n"), None);
    assert_eq!(blank_directives(".pragma\n"), None);
    assert_eq!(blank_directives(".import QtQuick 2.0\n"), None);
}

#[test]
fn a_directive_below_the_first_statement_stays() {
    assert_eq!(blank_directives("var x = 1\n.pragma library\n"), None);
}

#[test]
fn a_module_import_directive_becomes_an_import_symbol() {
    let results = extract(".import QtQuick 2.0 as QQ\nvar x = 1\n");

    let imports = imports(&results);
    assert_eq!(imports.len(), 1);
    let import = imports[0];
    assert_eq!(import.name, "QtQuick");
    assert_eq!(import.start_line, 1);
    assert_eq!(meta(import, "source"), "QtQuick");
    assert_eq!(meta(import, "source_kind"), "uri");
    assert_eq!(meta(import, "import_kind"), "module");
    assert_eq!(meta(import, "version"), "2.0");
    assert_eq!(meta(import, "alias"), "QQ");
    assert_eq!(meta(import, "local_name"), "QQ");
    assert_eq!(meta(import, "imported_name"), "QtQuick");
}

#[test]
fn a_javascript_file_import_directive_becomes_an_import_symbol() {
    let results = extract(".import \"Geometry.js\" as Geometry\nvar x = 1\n");

    let imports = imports(&results);
    assert_eq!(imports.len(), 1);
    let import = imports[0];
    assert_eq!(import.name, "Geometry.js");
    assert_eq!(meta(import, "source"), "Geometry.js");
    assert_eq!(meta(import, "source_kind"), "quoted");
    assert_eq!(meta(import, "import_kind"), "javascript");
    assert_eq!(meta(import, "alias"), "Geometry");
    assert_eq!(meta(import, "local_name"), "Geometry");
    assert_eq!(meta(import, "imported_name"), "Geometry.js");
}

#[test]
fn a_pragma_directive_becomes_a_qml_directive_fact() {
    let results = extract(".pragma library\nvar x = 1\n");

    let facts = directive_facts(&results);
    assert_eq!(facts.len(), 1);
    let fact = facts[0];
    assert_eq!(fact.language, "javascript");
    assert_eq!(fact.start_line, 1);
    assert_eq!(fact.start_column, 0);
    assert_eq!(fact_meta(fact, "directive"), "pragma");
    assert_eq!(fact_meta(fact, "name"), "library");
    assert!(imports(&results).is_empty());
}

#[test]
fn symbols_after_a_crlf_directive_keep_their_original_byte_offsets() {
    let source = ".pragma library\r\n\r\nfunction clamp(value) { return value }\r\n";

    let results = extract(source);

    let clamp = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "clamp")
        .expect("function after a directive should extract");
    assert_eq!(
        clamp.start_byte as usize,
        source.find("function clamp").expect("declaration offset")
    );
}

#[test]
fn a_directive_file_parses_without_syntax_errors() {
    let results = extract(".pragma library\n.import QtQuick 2.0 as QQ\nvar x = 1\n");

    assert_eq!(results.parse_diagnostics, Vec::new());
}

#[test]
fn a_directive_after_a_block_comment_on_the_same_line_is_blanked_in_place() {
    let source = "/* header */ .pragma library\nvar x = 1\n";

    let blanked = blank_directives(source).expect("directive lines should be blanked");

    assert_eq!(
        blanked,
        format!("/* header */ {}\nvar x = 1\n", " ".repeat(15))
    );
}

#[test]
fn a_directive_after_a_block_comment_that_ends_mid_line_is_blanked_in_place() {
    let source = "/* two\n   lines */ .pragma library\nvar x = 1\n";

    let blanked = blank_directives(source).expect("directive lines should be blanked");

    assert_eq!(
        blanked,
        format!("/* two\n   lines */ {}\nvar x = 1\n", " ".repeat(15))
    );
}

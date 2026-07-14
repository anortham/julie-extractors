use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::{ExtractionResults, SymbolKind, base::ParseDiagnosticKind};

const FIXTURE_PATH: &str = "fixtures/extraction/r/current_syntax/source.R";
const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/r/current_syntax/source.R");

fn extract(source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical(FIXTURE_PATH, source, Path::new("/tmp/current-r-syntax"))
        .expect("canonical R extraction should succeed")
}

fn parse(source: &str) -> Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_r::LANGUAGE.into())
        .expect("R grammar should load");
    parser.parse(source, None).expect("R source should parse")
}

fn find_node<'tree>(node: Node<'tree>, kind: &str, text: &str) -> Option<Node<'tree>> {
    if node.kind() == kind && node.utf8_text(FIXTURE_SOURCE.as_bytes()).ok() == Some(text) {
        return Some(node);
    }

    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| find_node(child, kind, text))
}

fn assert_symbol(results: &ExtractionResults, name: &str, kind: SymbolKind) {
    assert!(
        results
            .symbols
            .iter()
            .any(|symbol| symbol.name == name && symbol.kind == kind),
        "missing {kind:?} symbol {name}: {:#?}",
        results.symbols
    );
}

#[test]
fn r_130_fixture_parses_cleanly_with_current_nodes() {
    let results = extract(FIXTURE_SOURCE);
    assert!(
        results.parse_diagnostics.is_empty(),
        "expected zero parse diagnostics, got {}: {:#?}",
        results.parse_diagnostics.len(),
        results.parse_diagnostics
    );

    let tree = parse(FIXTURE_SOURCE);
    let root = tree.root_node();
    assert!(find_node(root, "identifier", "return").is_some());
    assert!(find_node(root, "float", "0x1.1p1").is_some());
    assert!(find_node(root, "identifier", "else_result").is_some());

    for (kind, text) in [
        ("string_open", "r\"("),
        ("string_content", "line one\r\nline two"),
        ("string_close", ")\""),
    ] {
        assert!(
            find_node(root, kind, text).is_some(),
            "expected {kind} node {text:?}: {}",
            root.to_sexp()
        );
    }

    let comment = find_node(root, "comment", "# comment boundary").expect("comment node");
    assert_eq!(
        comment.end_byte(),
        comment.start_byte() + "# comment boundary".len()
    );

    assert_symbol(&results, "return_identity", SymbolKind::Function);
    assert_symbol(&results, "hex_fraction", SymbolKind::Variable);
    assert_symbol(&results, "else_prefix", SymbolKind::Function);
    assert_symbol(&results, "else_result", SymbolKind::Variable);
    assert_symbol(&results, "raw_text", SymbolKind::Variable);
    assert_symbol(&results, "after_comment", SymbolKind::Variable);

    let return_identity = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "return_identity")
        .expect("return_identity function symbol");
    assert_eq!(
        return_identity.signature.as_deref(),
        Some("return_identity <- function(return)")
    );
}

#[test]
fn malformed_r_130_constructs_remain_diagnostic() {
    for (label, source) in [
        ("unclosed raw string", "value <- r\"(unterminated"),
        (
            "return parameter without a closing parenthesis",
            "value <- function(return",
        ),
    ] {
        let results = extract(source);
        assert!(
            results.parse_diagnostics.iter().any(|diagnostic| matches!(
                diagnostic.kind,
                ParseDiagnosticKind::Error | ParseDiagnosticKind::Missing
            )),
            "{label}: expected an error or missing diagnostic: {:#?}",
            results.parse_diagnostics
        );
    }
}

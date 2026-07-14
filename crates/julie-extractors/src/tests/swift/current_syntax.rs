use std::path::Path;

use tree_sitter::Tree;

use crate::{ExtractionResults, SymbolKind, base::ParseDiagnosticKind};

const FIXTURE_PATH: &str = "fixtures/extraction/swift/current_syntax/source.swift";
const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/swift/current_syntax/source.swift");

fn extract(source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical(FIXTURE_PATH, source, Path::new("/tmp/current-swift-syntax"))
        .expect("canonical Swift extraction should succeed")
}

fn parse(source: &str) -> Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .expect("Swift grammar should load");
    parser
        .parse(source, None)
        .expect("Swift source should parse")
}

fn tree_contains_kind(node: tree_sitter::Node<'_>, kind: &str) -> bool {
    if node.kind() == kind {
        return true;
    }

    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| tree_contains_kind(child, kind))
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

fn assert_diagnostic(label: &str, source: &str) {
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

#[test]
fn swift_073_fixture_parses_cleanly_with_current_nodes_and_canonical_rows() {
    let results = extract(FIXTURE_SOURCE);
    assert!(
        results.parse_diagnostics.is_empty(),
        "expected zero parse diagnostics, got {}: {:#?}",
        results.parse_diagnostics.len(),
        results.parse_diagnostics
    );

    let tree = parse(FIXTURE_SOURCE);
    for kind in [
        "consume_expression",
        "discard_statement",
        "throws_clause",
        "bracket_qualified_type",
        "directive",
    ] {
        assert!(
            tree_contains_kind(tree.root_node(), kind),
            "expected parser node {kind}: {}",
            tree.root_node().to_sexp()
        );
    }

    assert_symbol(&results, "OwnershipBox", SymbolKind::Struct);
    assert_symbol(&results, "take", SymbolKind::Method);
    assert_symbol(&results, "Registry", SymbolKind::Class);
    assert_symbol(&results, "shared", SymbolKind::Property);
    assert_symbol(&results, "nestedIndex", SymbolKind::Function);
    assert_symbol(&results, "formatter", SymbolKind::Property);
}

#[test]
fn malformed_swift_073_constructs_remain_diagnostic() {
    for (label, source) in [
        (
            "typed throws without a closing parenthesis",
            "func run() { do throws(SampleError { } catch { } }",
        ),
        (
            "bracket-qualified type without a nested name",
            "func nested(_ range: Range<[String].>) {}",
        ),
    ] {
        assert_diagnostic(label, source);
    }
}

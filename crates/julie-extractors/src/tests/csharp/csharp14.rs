use crate::{
    ExtractionResults,
    base::{IdentifierKind, ParseDiagnosticKind, RelationshipKind, SymbolKind},
};
use std::path::Path;

const FIXTURE_PATH: &str = "fixtures/extraction/csharp/csharp14/source.cs";
const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/csharp/csharp14/source.cs");

fn extract(source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical(
        FIXTURE_PATH,
        source,
        Path::new("/tmp/current-csharp14-syntax"),
    )
    .expect("canonical C# 14 extraction should succeed")
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

fn assert_identifier(results: &ExtractionResults, name: &str, kind: IdentifierKind) {
    assert!(
        results
            .identifiers
            .iter()
            .any(|identifier| identifier.name == name && identifier.kind == kind),
        "missing {kind:?} identifier {name}: {:#?}",
        results.identifiers
    );
}

fn assert_type(results: &ExtractionResults, symbol_name: &str, resolved_type: &str) {
    let symbol = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == symbol_name)
        .unwrap_or_else(|| panic!("missing symbol {symbol_name}"));
    assert!(
        results.types.values().any(|type_fact| {
            type_fact.symbol_id == symbol.id && type_fact.resolved_type == resolved_type
        }),
        "missing type {resolved_type} for {symbol_name}: {:#?}",
        results.types
    );
}

#[test]
fn csharp14_and_file_app_fixture_parses_cleanly_with_canonical_rows() {
    let results = extract(FIXTURE_SOURCE);

    assert!(
        results.parse_diagnostics.is_empty(),
        "expected zero parse diagnostics, got {}: {:#?}",
        results.parse_diagnostics.len(),
        results.parse_diagnostics
    );

    assert_symbol(&results, "EnumerableExtensions", SymbolKind::Class);
    assert_symbol(&results, "IsEmpty", SymbolKind::Property);
    assert_symbol(&results, "Where", SymbolKind::Method);
    assert_identifier(&results, "source", IdentifierKind::VariableRef);
    assert_identifier(&results, "Any", IdentifierKind::Call);
    assert_type(&results, "IsEmpty", "bool");
    assert_type(&results, "Where", "IEnumerable<TSource>");

    assert_identifier(&results, "Order", IdentifierKind::MemberAccess);
    assert_identifier(&results, "GetCurrentOrder", IdentifierKind::Call);
    let current_order = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "GetCurrentOrder")
        .expect("GetCurrentOrder symbol");
    assert!(results.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::Calls
            && relationship.to_symbol_id == current_order.id
    }));

    assert_identifier(&results, "List", IdentifierKind::TypeUsage);
    assert_identifier(&results, "nameof", IdentifierKind::Call);

    assert_symbol(&results, "Message", SymbolKind::Property);
    assert_identifier(&results, "value", IdentifierKind::VariableRef);
    assert_type(&results, "Message", "string");

    assert_symbol(&results, "parse$lambda", SymbolKind::Function);
    assert_identifier(&results, "text", IdentifierKind::VariableRef);
    assert_identifier(&results, "result", IdentifierKind::VariableRef);
    assert_identifier(&results, "TryParse", IdentifierKind::Call);

    assert!(
        results
            .symbols
            .iter()
            .filter(|symbol| {
                symbol.name == "FileAppCustomer" && symbol.kind == SymbolKind::Constructor
            })
            .count()
            >= 2
    );
    assert_symbol(&results, "Changed", SymbolKind::Event);
    assert!(
        results
            .relationships
            .iter()
            .filter(|relationship| relationship.kind == RelationshipKind::References)
            .count()
            >= 2
    );

    assert_symbol(&results, "MutableCounter", SymbolKind::Class);
    assert_symbol(&results, "operator +=", SymbolKind::Method);
    assert_identifier(&results, "counter", IdentifierKind::VariableRef);
    assert_identifier(&results, "Value", IdentifierKind::VariableRef);
    assert_identifier(&results, "amount", IdentifierKind::VariableRef);
}

#[test]
fn malformed_csharp14_extension_declaration_remains_diagnostic() {
    assert_diagnostic(
        "malformed extension declaration",
        r#"
using System.Collections.Generic;

public static class BrokenExtensions
{
    extension<T>(IEnumerable<T> source
    {
        public bool IsEmpty => false;
    }
}
"#,
    );
}

#[test]
fn file_app_directive_without_payload_remains_diagnostic() {
    assert_diagnostic(
        "file-app directive without payload",
        "#:package\nConsole.WriteLine(\"invalid\");\n",
    );
}

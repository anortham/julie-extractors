use crate::base::{ExtractionResults, Identifier, IdentifierKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(file_path: &str, source: &str) -> ExtractionResults {
    extract_canonical(file_path, source, Path::new("/repo")).expect("XML extraction")
}

fn identifier_at(results: &ExtractionResults, start: usize, kind: IdentifierKind) -> &Identifier {
    results
        .identifiers
        .iter()
        .find(|identifier| identifier.start_byte as usize == start && identifier.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} identifier at byte {start}"))
}

fn access(identifier: &Identifier) -> Option<&str> {
    identifier
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("access"))
        .and_then(serde_json::Value::as_str)
}

#[test]
fn msbuild_property_substitutions_are_read_only_and_target_calls_have_no_access() {
    let source = concat!(
        "<Project>\n",
        "  <PropertyGroup><Version>4.2</Version></PropertyGroup>\n",
        "  <Target Name=\"Build\"><Message Text=\"$(Version)\" />",
        "<CallTarget Targets=\"ComputeVersion\" /></Target>\n",
        "  <Target Name=\"ComputeVersion\" />\n",
        "</Project>\n",
    );
    let results = extract("src/App.csproj", source);

    assert!(results.parse_diagnostics.is_empty());

    let property = identifier_at(
        &results,
        source.find("$(Version)").unwrap() + 2,
        IdentifierKind::VariableRef,
    );
    assert_eq!(access(property), Some("read"));
    assert_eq!(property.language, "xml");
    assert_eq!(
        &source[property.start_byte as usize..property.end_byte as usize],
        "Version"
    );

    let target_call = identifier_at(
        &results,
        source.find("ComputeVersion").unwrap(),
        IdentifierKind::Call,
    );
    assert_eq!(access(target_call), None);
    assert_eq!(target_call.language, "xml");
    assert_eq!(
        &source[target_call.start_byte as usize..target_call.end_byte as usize],
        "ComputeVersion"
    );
}

#[test]
fn schema_qname_type_usage_does_not_receive_access_metadata() {
    let source = "<xs:element xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" name=\"number\" type=\"xs:string\"/>\n";
    let results = extract("schema.xsd", source);

    assert!(results.parse_diagnostics.is_empty());

    let type_usage = identifier_at(
        &results,
        source.find("string").unwrap(),
        IdentifierKind::TypeUsage,
    );
    assert_eq!(access(type_usage), None);
    assert_eq!(type_usage.language, "xml");
    assert_eq!(
        &source[type_usage.start_byte as usize..type_usage.end_byte as usize],
        "string"
    );
    assert_eq!(
        type_usage
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("qname"))
            .and_then(serde_json::Value::as_str),
        Some("xs:string")
    );
}

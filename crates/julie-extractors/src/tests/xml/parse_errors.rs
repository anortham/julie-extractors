use crate::base::ParseDiagnosticKind;
use crate::pipeline::extract_canonical;
use std::path::PathBuf;

const BROKEN: &str = r#"<catalog name="parts">
  <part name="bolt" type="xs:string">
  <part name="nut"/>
</catalog>
"#;

fn extract(code: &str) -> crate::ExtractionResults {
    extract_canonical("catalog.xml", code, &PathBuf::from("/tmp/test")).expect("extraction failed")
}

#[test]
fn malformed_documents_still_yield_the_elements_that_parsed() {
    let results = extract(BROKEN);
    let names: Vec<_> = results
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();

    assert_eq!(names, vec!["parts", "bolt", "nut"]);
}

#[test]
fn malformed_documents_report_parse_diagnostics() {
    let results = extract(BROKEN);

    assert!(
        results.parse_diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            ParseDiagnosticKind::Error | ParseDiagnosticKind::Missing
        )),
        "expected an error or missing diagnostic, got {:?}",
        results.parse_diagnostics
    );
}

#[test]
fn clean_documents_report_no_diagnostics() {
    let results = extract("<catalog name=\"parts\"><part name=\"bolt\"/></catalog>\n");

    assert!(
        results.parse_diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        results.parse_diagnostics
    );
}

use crate::base::ParseDiagnosticKind;
use crate::pipeline::extract_canonical;
use std::path::PathBuf;

const BROKEN: &str = r#"-module(bank).
-export([open/1]).

open(Id) ->
    #account{id = Id}.

broken(( ->

audit(A) ->
    A.
"#;

fn extract(code: &str) -> crate::ExtractionResults {
    extract_canonical("bank.erl", code, &PathBuf::from("/tmp/test")).expect("extraction failed")
}

#[test]
fn parse_errors_still_yield_the_declarations_that_parsed() {
    let results = extract(BROKEN);
    let names: Vec<_> = results
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();

    assert!(names.contains(&"bank"), "got {names:?}");
    assert!(names.contains(&"open"), "got {names:?}");
}

#[test]
fn parse_errors_are_reported_as_diagnostics() {
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
fn clean_sources_report_no_diagnostics() {
    let results = extract("-module(bank).\n-export([open/1]).\nopen(Id) -> Id.\n");

    assert!(
        results.parse_diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        results.parse_diagnostics
    );
}

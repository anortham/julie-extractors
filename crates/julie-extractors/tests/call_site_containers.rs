use std::path::Path;

use julie_extractors::{ExtractionResults, extract_canonical};

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/repo")).expect("extraction should succeed")
}

fn symbol_name(results: &ExtractionResults, id: Option<&str>) -> Option<String> {
    let id = id?;
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
}

fn pending_site_containers(
    results: &ExtractionResults,
    source: &str,
) -> Vec<(String, Option<String>, Option<String>)> {
    results
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.reference_site_is_exact)
        .map(|pending| {
            let span = pending
                .span
                .expect("exact pending call should carry a span");
            let identifier = results
                .identifiers
                .iter()
                .find(|identifier| {
                    identifier.start_byte == span.start_byte && identifier.end_byte == span.end_byte
                })
                .expect("exact pending call should share its site with an identifier");
            let site_container = pending
                .caller_scope_symbol_id
                .as_deref()
                .or(Some(pending.pending.from_symbol_id.as_str()));
            (
                source[span.start_byte as usize..span.end_byte as usize].to_owned(),
                symbol_name(results, identifier.containing_symbol_id.as_deref()),
                symbol_name(results, site_container),
            )
        })
        .collect()
}

#[test]
fn ginkgo_calls_run_in_the_block_around_the_node_they_declare() {
    let source = r#"package calc

import . "github.com/onsi/ginkgo/v2"

var _ = Describe("calculator", func() {
	Context("addition", func() {
		BeforeEach(func() {})
		AfterEach(func() {})
	})
})
"#;
    let results = extract("calc_test.go", source);
    let containers = pending_site_containers(&results, source);

    for (callee, identifier_container, site_container) in &containers {
        assert_eq!(identifier_container, site_container, "{callee}");
    }
    let container_of = |callee: &str| {
        containers
            .iter()
            .find(|(text, _, _)| text == callee)
            .and_then(|(_, container, _)| container.clone())
    };
    assert_eq!(container_of("Context").as_deref(), Some("calculator"));
    assert_eq!(container_of("AfterEach").as_deref(), Some("addition"));
}

#[test]
fn qml_binding_calls_share_the_bound_property_as_site_container() {
    let source = r#"import QtQuick

Item {
    readonly property color tint: Qt.rgba(0.1, 0.2, 0.3, 1)
    Loader { source: Qt.resolvedUrl("pages/Settings.qml") }
}
"#;
    let results = extract("Dashboard.qml", source);
    let containers = pending_site_containers(&results, source);

    assert!(!containers.is_empty());
    for (callee, identifier_container, site_container) in &containers {
        assert_eq!(identifier_container, site_container, "{callee}");
    }
    assert!(
        containers
            .iter()
            .any(|(callee, container, _)| callee == "rgba" && container.as_deref() == Some("tint"))
    );
}

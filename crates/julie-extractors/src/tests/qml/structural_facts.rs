use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/qml/basic/source.qml");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/qml/basic/source.qml",
        source,
        Path::new("/repo"),
    )
    .expect("canonical QML extraction should succeed")
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

#[test]
fn qml_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "qml.import_statement.v1",
        "qml.property_declaration.v1",
        "qml.signal_declaration.v1",
        "qml.binding.v1",
        "qml.object_instantiation.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let import = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "qml.import_statement.v1")
        .expect("expected import statement fact");
    assert_eq!(metadata_str(import, "import_module"), Some("QtQuick"));

    let title_property = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "qml.property_declaration.v1"
                && metadata_str(fact, "property_name") == Some("title")
        })
        .expect("expected title property fact");
    assert_eq!(
        metadata_str(title_property, "property_type"),
        Some("string")
    );

    let signal = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "qml.signal_declaration.v1")
        .expect("expected signal declaration fact");
    assert_eq!(metadata_str(signal, "signal_name"), Some("activated"));

    let binding = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "qml.binding.v1"
                && metadata_str(fact, "property_name") == Some("text")
        })
        .expect("expected text binding fact");
    assert_ne!(
        metadata_str(binding, "property_name"),
        Some("id"),
        "id bindings must not be classified as semantic property bindings"
    );
}

#[test]
fn qml_import_facts_publish_normalized_binding_fields() {
    let source = r#"
import QtQuick 2.15
import "components" as Components
import "./js/helpers.js" as Helpers
Item {}
"#;
    let results = extract(source);
    let imports = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "qml.import_statement.v1")
        .collect::<Vec<_>>();

    assert_eq!(metadata_str(imports[0], "source"), Some("QtQuick"));
    assert_eq!(metadata_str(imports[0], "version"), Some("2.15"));
    assert_eq!(metadata_str(imports[1], "source"), Some("components"));
    assert_eq!(metadata_str(imports[1], "alias"), Some("Components"));
    assert_eq!(metadata_str(imports[1], "local_name"), Some("Components"));
    assert_eq!(
        metadata_str(imports[1], "imported_name"),
        Some("components")
    );
    assert_eq!(metadata_str(imports[2], "source"), Some("./js/helpers.js"));
    assert_eq!(metadata_str(imports[2], "alias"), Some("Helpers"));
}

#[test]
fn qml_object_facts_publish_component_type_names() {
    let results = extract(
        r#"
import QtQuick 2.15

Item {
    Card {}
}
"#,
    );
    let object = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "qml.object_instantiation.v1"
                && metadata_str(fact, "type_name") == Some("Card")
        })
        .expect("nested object instantiation fact");
    assert_eq!(metadata_str(object, "type_name"), Some("Card"));
}

#[test]
fn qmltypes_facts_publish_declaration_roles() {
    let results = crate::pipeline::extract_canonical(
        "fixtures/extraction/qml/typeinfo/source.qmltypes",
        "Module { Component { name: \"Widget\" } }",
        Path::new("/repo"),
    )
    .expect("canonical qmltypes extraction should succeed");
    let declaration = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "qml.typeinfo_declaration.v1")
        .expect("qmltypes declaration fact");
    assert_eq!(metadata_str(declaration, "type_name"), Some("Module"));
    assert_eq!(metadata_str(declaration, "typeinfo_kind"), Some("module"));
}

#[test]
fn qml_binding_skips_id_and_signal_handler_bindings() {
    let source = r#"
import QtQuick 2.15

Item {
    id: root
    property string title: "Worker"

    MouseArea {
        onClicked: { root.title = "clicked" }
    }
}
"#;
    let results = extract(source);
    let binding_names = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "qml.binding.v1")
        .filter_map(|fact| metadata_str(fact, "property_name"))
        .collect::<Vec<_>>();
    assert!(
        !binding_names.contains(&"id"),
        "id bindings must not emit qml.binding.v1"
    );
    assert!(
        !binding_names.iter().any(|name| name.starts_with("on")),
        "signal handler bindings must not emit qml.binding.v1"
    );
}

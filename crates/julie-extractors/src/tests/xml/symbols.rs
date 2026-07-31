use super::support::{extract, find, names};
use crate::base::{SymbolKind, Visibility};
use serde_json::Value;

const SCHEMA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="AddPhone">
    <xs:sequence>
      <xs:element name="number" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
</xs:schema>
"#;

#[test]
fn name_attribute_promotes_an_element_to_a_symbol() {
    let symbols = extract(SCHEMA);
    let add_phone = find(&symbols, "AddPhone");

    assert_eq!(add_phone.kind, SymbolKind::Module);
    assert_eq!(
        add_phone.signature.as_deref(),
        Some("<xs:complexType name=\"AddPhone\">")
    );
    assert_eq!(add_phone.visibility, Some(Visibility::Public));
    assert_eq!(add_phone.parent_id, None);
}

#[test]
fn id_attribute_promotes_an_element_to_a_symbol() {
    let symbols = extract("<doc><section id=\"intro\">Hello</section></doc>\n");

    assert_eq!(names(&symbols), vec!["intro"]);
    assert_eq!(symbols[0].kind, SymbolKind::Variable);
}

#[test]
fn name_wins_over_id_when_an_element_carries_both() {
    let symbols = extract("<doc><entry id=\"e1\" name=\"primary\"/></doc>\n");

    assert_eq!(names(&symbols), vec!["primary"]);
    assert_eq!(
        symbols[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("name_attribute")),
        Some(&Value::String("name".to_string()))
    );
}

#[test]
fn anonymous_elements_emit_no_symbols() {
    let symbols = extract("<root><item/><row><cell/></row></root>\n");

    assert!(symbols.is_empty(), "got {:?}", names(&symbols));
}

#[test]
fn named_elements_chain_through_the_nearest_named_ancestor() {
    let symbols = extract(SCHEMA);
    let add_phone_id = find(&symbols, "AddPhone").id.clone();

    assert_eq!(
        find(&symbols, "number").parent_id.as_deref(),
        Some(add_phone_id.as_str()),
        "xs:sequence is anonymous, so number chains to AddPhone"
    );
}

#[test]
fn container_elements_are_modules_and_leaf_elements_are_variables() {
    let symbols = extract(SCHEMA);

    assert_eq!(find(&symbols, "AddPhone").kind, SymbolKind::Module);
    assert_eq!(find(&symbols, "number").kind, SymbolKind::Variable);
}

#[test]
fn an_element_whose_only_children_are_text_is_a_variable() {
    let symbols = extract("<root name=\"cfg\">plain text</root>\n");

    assert_eq!(find(&symbols, "cfg").kind, SymbolKind::Variable);
}

#[test]
fn an_empty_name_attribute_does_not_promote_an_element() {
    let symbols = extract("<root><entry name=\"\"/><entry name=\"  \"/></root>\n");

    assert!(symbols.is_empty(), "got {:?}", names(&symbols));
}

#[test]
fn symbol_metadata_records_the_tag_and_the_promoting_attribute() {
    let symbols = extract(SCHEMA);
    let metadata = find(&symbols, "AddPhone")
        .metadata
        .as_ref()
        .expect("metadata");

    assert_eq!(
        metadata.get("tag"),
        Some(&Value::String("xs:complexType".to_string()))
    );
    assert_eq!(
        metadata.get("name_attribute"),
        Some(&Value::String("name".to_string()))
    );
}

#[test]
fn wsdl_service_definitions_promote_service_port_and_operation() {
    let symbols = extract(
        r#"<definitions name="PhoneBook">
  <portType name="PhoneBookPort">
    <operation name="AddPhone"/>
  </portType>
</definitions>
"#,
    );

    assert_eq!(
        names(&symbols),
        vec!["PhoneBook", "PhoneBookPort", "AddPhone"]
    );
    let port_type_id = find(&symbols, "PhoneBookPort").id.clone();
    assert_eq!(
        find(&symbols, "AddPhone").parent_id.as_deref(),
        Some(port_type_id.as_str())
    );
}

#[test]
fn body_hash_ignores_comment_edits() {
    let with_comment = extract(
        r#"<root name="cfg">
  <!-- explains the entry -->
  <entry name="a"/>
</root>
"#,
    );
    let without_comment = extract(
        r#"<root name="cfg">
  <entry name="a"/>
</root>
"#,
    );

    assert_eq!(
        find(&with_comment, "cfg").body_hash,
        find(&without_comment, "cfg").body_hash
    );
    assert!(find(&with_comment, "cfg").body_hash.is_some());
}

use super::support::{extract_identifiers, find};
use crate::base::IdentifierKind;

fn reference_names(code: &str) -> Vec<String> {
    let (_, identifiers) = extract_identifiers(code);
    identifiers
        .into_iter()
        .map(|identifier| identifier.name)
        .collect()
}

#[test]
fn type_attribute_emits_a_qname_type_usage_identifier() {
    let (_, identifiers) =
        extract_identifiers("<xs:element name=\"number\" type=\"xs:string\"/>\n");

    assert_eq!(identifiers.len(), 1);
    assert_eq!(identifiers[0].name, "xs:string");
    assert_eq!(identifiers[0].kind, IdentifierKind::TypeUsage);
}

#[test]
fn ref_attribute_emits_a_qname_type_usage_identifier() {
    assert_eq!(
        reference_names("<xs:element ref=\"tns:Other\"/>\n"),
        vec!["tns:Other".to_string()]
    );
}

#[test]
fn base_attribute_emits_a_qname_type_usage_identifier() {
    assert_eq!(
        reference_names("<xs:extension base=\"tns:AddPhone\"/>\n"),
        vec!["tns:AddPhone".to_string()]
    );
}

#[test]
fn element_attribute_emits_a_qname_type_usage_identifier() {
    assert_eq!(
        reference_names("<part name=\"entry\" element=\"tns:AddPhone\"/>\n"),
        vec!["tns:AddPhone".to_string()]
    );
}

#[test]
fn qname_values_are_recorded_exactly_as_written_without_namespace_resolution() {
    assert_eq!(
        reference_names("<xs:element type=\"UnprefixedType\"/>\n"),
        vec!["UnprefixedType".to_string()]
    );
}

#[test]
fn references_bind_to_the_containing_named_element() {
    let (symbols, identifiers) = extract_identifiers(
        r#"<xs:complexType name="AddPhone">
  <xs:sequence>
    <xs:element name="number" type="xs:string"/>
  </xs:sequence>
</xs:complexType>
"#,
    );

    let number_id = find(&symbols, "number").id.clone();
    let reference = identifiers
        .iter()
        .find(|identifier| identifier.name == "xs:string")
        .expect("xs:string reference");

    assert_eq!(
        reference.containing_symbol_id.as_deref(),
        Some(number_id.as_str())
    );
}

#[test]
fn non_reference_attributes_emit_no_identifiers() {
    let (_, identifiers) =
        extract_identifiers("<dependency scope=\"test\" id=\"tempfile\">tempfile</dependency>\n");

    assert!(identifiers.is_empty(), "got {identifiers:?}");
}

#[test]
fn empty_reference_attributes_emit_no_identifiers() {
    let (_, identifiers) = extract_identifiers("<xs:element name=\"n\" type=\"\"/>\n");

    assert!(identifiers.is_empty(), "got {identifiers:?}");
}

#[test]
fn prefixed_reference_attributes_match_on_their_local_name() {
    assert_eq!(
        reference_names("<entry xsi:type=\"tns:Concrete\"/>\n"),
        vec!["tns:Concrete".to_string()]
    );
}

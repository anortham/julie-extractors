use super::support::{extract_identifiers, find};
use crate::base::IdentifierKind;

/// Real schema and service documents always declare the namespace their
/// elements live in, and that declaration is what marks an attribute value as a
/// component reference. Snippets carry it for the same reason.
const XSD: &str = "xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"";
const WSDL: &str = "xmlns=\"http://schemas.xmlsoap.org/wsdl/\"";

fn reference_names(code: &str) -> Vec<String> {
    let (_, identifiers) = extract_identifiers(code);
    identifiers
        .into_iter()
        .map(|identifier| identifier.name)
        .collect()
}

#[test]
fn type_attribute_emits_a_qname_type_usage_identifier() {
    let (_, identifiers) = extract_identifiers(&format!(
        "<xs:element {XSD} name=\"number\" type=\"xs:string\"/>\n"
    ));

    assert_eq!(identifiers.len(), 1);
    assert_eq!(identifiers[0].name, "xs:string");
    assert_eq!(identifiers[0].kind, IdentifierKind::TypeUsage);
}

#[test]
fn ref_attribute_emits_a_qname_type_usage_identifier() {
    assert_eq!(
        reference_names(&format!("<xs:element {XSD} ref=\"tns:Other\"/>\n")),
        vec!["tns:Other".to_string()]
    );
}

#[test]
fn base_attribute_emits_a_qname_type_usage_identifier() {
    assert_eq!(
        reference_names(&format!("<xs:extension {XSD} base=\"tns:AddPhone\"/>\n")),
        vec!["tns:AddPhone".to_string()]
    );
}

#[test]
fn element_attribute_emits_a_qname_type_usage_identifier() {
    assert_eq!(
        reference_names(&format!(
            "<part {WSDL} name=\"entry\" element=\"tns:AddPhone\"/>\n"
        )),
        vec!["tns:AddPhone".to_string()]
    );
}

#[test]
fn qname_values_are_recorded_exactly_as_written_without_namespace_resolution() {
    assert_eq!(
        reference_names(&format!("<xs:element {XSD} type=\"UnprefixedType\"/>\n")),
        vec!["UnprefixedType".to_string()]
    );
}

#[test]
fn references_bind_to_the_containing_named_element() {
    let (symbols, identifiers) = extract_identifiers(&format!(
        r#"<xs:complexType {XSD} name="AddPhone">
  <xs:sequence>
    <xs:element name="number" type="xs:string"/>
  </xs:sequence>
</xs:complexType>
"#
    ));

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
    let (_, identifiers) =
        extract_identifiers(&format!("<xs:element {XSD} name=\"n\" type=\"\"/>\n"));

    assert!(identifiers.is_empty(), "got {identifiers:?}");
}

/// `xsi:type` names a schema type wherever it appears, including on an instance
/// document element that is not itself a schema component.
#[test]
fn prefixed_reference_attributes_match_on_their_local_name() {
    assert_eq!(
        reference_names(
            "<entry xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"tns:Concrete\"/>\n"
        ),
        vec!["tns:Concrete".to_string()]
    );
}

#[test]
fn generic_documents_emit_no_type_usage_from_reference_named_attributes() {
    let (_, identifiers) = extract_identifiers(
        r#"<configuration name="phonebook">
  <add id="ConnectionTimeout" type="xs:int">30</add>
  <button type="button" ref="submit" />
  <link base="/api" element="anchor" />
</configuration>
"#,
    );

    assert!(
        identifiers.is_empty(),
        "no namespace declares these elements as schema components; got {identifiers:?}"
    );
}

#[test]
fn an_undeclared_prefix_does_not_make_an_element_a_schema_component() {
    let (_, identifiers) =
        extract_identifiers("<xs:element name=\"number\" type=\"xs:string\"/>\n");

    assert!(
        identifiers.is_empty(),
        "the xs prefix is bound to nothing here; got {identifiers:?}"
    );
}

#[test]
fn a_schema_namespace_declared_on_an_ancestor_still_qualifies_its_elements() {
    assert_eq!(
        reference_names(&format!(
            r#"<xs:schema {XSD}>
  <xs:element name="number" type="xs:string"/>
</xs:schema>
"#
        )),
        vec!["xs:string".to_string()]
    );
}

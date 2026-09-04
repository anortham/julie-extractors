use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical XML extraction should succeed")
}

fn only_fact<'a>(results: &'a crate::ExtractionResults, pattern_id: &str) -> &'a StructuralFact {
    let facts = facts_with_pattern(results, pattern_id);
    assert_eq!(
        facts.len(),
        1,
        "expected exactly one {pattern_id} fact, got {}",
        facts.len()
    );
    facts[0]
}

fn metadata_u64(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_u64())
}

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
}

fn pattern_ids(results: &crate::ExtractionResults) -> BTreeSet<&str> {
    results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect()
}

const CONFIG: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<configuration name="phonebook">
  <appSettings>
    <add id="Timeout">30</add>
  </appSettings>
</configuration>
"#;

const SCHEMA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:tns="urn:phonebook"
           targetNamespace="urn:phonebook">
  <xs:import namespace="urn:phonebook-common" schemaLocation="common.xsd" />
  <xs:include schemaLocation="phonebook-base.xsd" />

  <xs:simpleType name="PhoneNumber">
    <xs:restriction base="xs:string" />
  </xs:simpleType>

  <xs:complexType name="AddPhone">
    <xs:sequence>
      <xs:element name="owner" type="xs:string" />
    </xs:sequence>
  </xs:complexType>

  <xs:complexType name="AddMobilePhone">
    <xs:complexContent>
      <xs:extension base="tns:AddPhone">
        <xs:sequence>
          <xs:element name="carrier" type="xs:string" />
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>

  <xs:element name="AddPhoneRequest" type="tns:AddPhone" />
</xs:schema>
"#;

const SERVICE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions name="PhoneBookService"
             xmlns="http://schemas.xmlsoap.org/wsdl/"
             xmlns:tns="urn:phonebook">
  <message name="AddPhoneRequest">
    <part name="body" element="tns:AddPhone" />
  </message>

  <portType name="PhoneBookPort">
    <operation name="AddPhone">
      <input message="tns:AddPhoneRequest" />
      <output message="tns:AddPhoneResponse" />
    </operation>
  </portType>

  <binding name="PhoneBookBinding" type="tns:PhoneBookPort">
    <operation name="AddPhone">
      <input />
    </operation>
  </binding>

  <service name="PhoneBook">
    <port name="PhoneBookPort" binding="tns:PhoneBookBinding" />
  </service>
</definitions>
"#;

#[test]
fn every_xml_document_emits_one_document_fact() {
    let results = extract("app.config.xml", CONFIG);
    let document = only_fact(&results, "xml.document.v1");

    assert_eq!(document.capture_name, "document");
    assert_eq!(document.node_kind, "document");
    assert_eq!(metadata_str(document, "dialect"), Some("xml"));
    assert_eq!(
        metadata_str(document, "root_element"),
        Some("configuration")
    );
    assert_eq!(metadata_bool(document, "has_xml_declaration"), Some(true));
    assert_eq!(metadata_u64(document, "element_count"), Some(3));
    assert_eq!(metadata_u64(document, "max_depth"), Some(3));
    assert_eq!(metadata_u64(document, "namespace_count"), Some(0));
}

#[test]
fn a_document_without_a_declaration_records_it() {
    let results = extract("fragment.xml", "<root><leaf /></root>");
    let document = only_fact(&results, "xml.document.v1");

    assert_eq!(metadata_bool(document, "has_xml_declaration"), Some(false));
    assert_eq!(metadata_u64(document, "element_count"), Some(2));
    assert_eq!(metadata_u64(document, "max_depth"), Some(2));
}

#[test]
fn a_document_without_a_root_element_emits_no_document_fact() {
    let results = extract("empty.xml", "<!-- nothing here -->\n");

    assert!(facts_with_pattern(&results, "xml.document.v1").is_empty());
}

#[test]
fn namespace_declarations_separate_prefixed_and_default_bindings() {
    let results = extract("service.wsdl", SERVICE);
    let namespaces = facts_with_pattern(&results, "xml.namespace_declaration.v1");

    assert_eq!(namespaces.len(), 2);

    let default = namespaces
        .iter()
        .find(|fact| metadata_bool(fact, "is_default") == Some(true))
        .expect("default namespace declaration");
    assert_eq!(
        metadata_str(default, "namespace_uri"),
        Some("http://schemas.xmlsoap.org/wsdl/")
    );
    assert_eq!(metadata_str(default, "prefix"), None);

    let prefixed = namespaces
        .iter()
        .find(|fact| metadata_bool(fact, "is_default") == Some(false))
        .expect("prefixed namespace declaration");
    assert_eq!(metadata_str(prefixed, "prefix"), Some("tns"));
    assert_eq!(
        metadata_str(prefixed, "namespace_uri"),
        Some("urn:phonebook")
    );
    assert_eq!(prefixed.node_kind, "Attribute");
}

#[test]
fn schema_documents_emit_named_type_declarations_with_raw_base_qnames() {
    let results = extract("phonebook.xsd", SCHEMA);
    let types = facts_with_pattern(&results, "xml.xsd.type.v1");

    assert_eq!(
        types
            .iter()
            .filter_map(|fact| metadata_str(fact, "type_name"))
            .collect::<Vec<_>>(),
        ["PhoneNumber", "AddPhone", "AddMobilePhone"]
    );

    let simple = types
        .iter()
        .find(|fact| metadata_str(fact, "type_name") == Some("PhoneNumber"))
        .unwrap();
    assert_eq!(metadata_str(simple, "type_kind"), Some("simple"));
    assert_eq!(metadata_str(simple, "base_type"), Some("xs:string"));

    let plain = types
        .iter()
        .find(|fact| metadata_str(fact, "type_name") == Some("AddPhone"))
        .unwrap();
    assert_eq!(metadata_str(plain, "type_kind"), Some("complex"));
    assert_eq!(metadata_str(plain, "base_type"), None);

    let derived = types
        .iter()
        .find(|fact| metadata_str(fact, "type_name") == Some("AddMobilePhone"))
        .unwrap();
    assert_eq!(metadata_str(derived, "base_type"), Some("tns:AddPhone"));
}

#[test]
fn schema_documents_emit_only_top_level_element_declarations() {
    let results = extract("phonebook.xsd", SCHEMA);
    let element = only_fact(&results, "xml.xsd.element.v1");

    assert_eq!(
        metadata_str(element, "element_name"),
        Some("AddPhoneRequest")
    );
    assert_eq!(metadata_str(element, "type_ref"), Some("tns:AddPhone"));
}

#[test]
fn schema_documents_emit_imports_and_includes_with_their_locations() {
    let results = extract("phonebook.xsd", SCHEMA);
    let imports = facts_with_pattern(&results, "xml.xsd.import.v1");

    assert_eq!(imports.len(), 2);

    let import = imports
        .iter()
        .find(|fact| metadata_str(fact, "import_kind") == Some("import"))
        .expect("xs:import fact");
    assert_eq!(
        metadata_str(import, "namespace"),
        Some("urn:phonebook-common")
    );
    assert_eq!(metadata_str(import, "schema_location"), Some("common.xsd"));

    let include = imports
        .iter()
        .find(|fact| metadata_str(fact, "import_kind") == Some("include"))
        .expect("xs:include fact");
    assert_eq!(metadata_str(include, "namespace"), None);
    assert_eq!(
        metadata_str(include, "schema_location"),
        Some("phonebook-base.xsd")
    );
}

#[test]
fn service_documents_emit_services_ports_messages_and_bindings() {
    let results = extract("service.wsdl", SERVICE);

    let service = only_fact(&results, "xml.wsdl.service.v1");
    assert_eq!(metadata_str(service, "service_name"), Some("PhoneBook"));
    assert_eq!(metadata_u64(service, "port_count"), Some(1));

    let port = only_fact(&results, "xml.wsdl.port.v1");
    assert_eq!(metadata_str(port, "port_name"), Some("PhoneBookPort"));
    assert_eq!(metadata_str(port, "binding"), Some("tns:PhoneBookBinding"));

    let message = only_fact(&results, "xml.wsdl.message.v1");
    assert_eq!(
        metadata_str(message, "message_name"),
        Some("AddPhoneRequest")
    );
    assert_eq!(metadata_u64(message, "part_count"), Some(1));

    let binding = only_fact(&results, "xml.wsdl.binding.v1");
    assert_eq!(
        metadata_str(binding, "binding_name"),
        Some("PhoneBookBinding")
    );
    assert_eq!(
        metadata_str(binding, "port_type"),
        Some("tns:PhoneBookPort")
    );
}

#[test]
fn service_operations_record_their_owner_and_message_qnames() {
    let results = extract("service.wsdl", SERVICE);
    let operations = facts_with_pattern(&results, "xml.wsdl.operation.v1");

    assert_eq!(operations.len(), 2);

    let abstract_operation = operations
        .iter()
        .find(|fact| metadata_str(fact, "parent_kind") == Some("port_type"))
        .expect("portType operation");
    assert_eq!(
        metadata_str(abstract_operation, "operation_name"),
        Some("AddPhone")
    );
    assert_eq!(
        metadata_str(abstract_operation, "parent_name"),
        Some("PhoneBookPort")
    );
    assert_eq!(
        metadata_str(abstract_operation, "input_message"),
        Some("tns:AddPhoneRequest")
    );
    assert_eq!(
        metadata_str(abstract_operation, "output_message"),
        Some("tns:AddPhoneResponse")
    );

    let concrete_operation = operations
        .iter()
        .find(|fact| metadata_str(fact, "parent_kind") == Some("binding"))
        .expect("binding operation");
    assert_eq!(
        metadata_str(concrete_operation, "parent_name"),
        Some("PhoneBookBinding")
    );
    assert_eq!(metadata_str(concrete_operation, "input_message"), None);
    assert_eq!(metadata_str(concrete_operation, "output_message"), None);
}

#[test]
fn schema_facts_are_dialect_scoped_to_their_extension() {
    let as_plain_xml = extract("phonebook.xml", SCHEMA);
    let as_schema = extract("phonebook.xsd", SCHEMA);

    assert!(facts_with_pattern(&as_plain_xml, "xml.xsd.type.v1").is_empty());
    assert!(!facts_with_pattern(&as_schema, "xml.xsd.type.v1").is_empty());
    assert_eq!(
        metadata_str(only_fact(&as_plain_xml, "xml.document.v1"), "dialect"),
        Some("xml")
    );
    assert_eq!(
        metadata_str(only_fact(&as_schema, "xml.document.v1"), "dialect"),
        Some("xsd")
    );
}

#[test]
fn service_facts_are_dialect_scoped_to_their_extension() {
    let as_plain_xml = extract("service.xml", SERVICE);
    let as_service = extract("service.wsdl", SERVICE);

    assert!(facts_with_pattern(&as_plain_xml, "xml.wsdl.service.v1").is_empty());
    assert!(!facts_with_pattern(&as_service, "xml.wsdl.service.v1").is_empty());
    assert_eq!(
        metadata_str(only_fact(&as_service, "xml.document.v1"), "dialect"),
        Some("wsdl")
    );
}

#[test]
fn schema_components_are_matched_on_their_local_name() {
    let prefixed = r#"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <xsd:complexType name="Order" />
</xsd:schema>
"#;
    let results = extract("orders.xsd", prefixed);
    let declared = only_fact(&results, "xml.xsd.type.v1");

    assert_eq!(metadata_str(declared, "type_name"), Some("Order"));
    assert_eq!(metadata_str(declared, "type_kind"), Some("complex"));
}

#[test]
fn anonymous_schema_components_emit_nothing() {
    let anonymous = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Order">
    <xs:complexType>
      <xs:sequence />
    </xs:complexType>
  </xs:element>
</xs:schema>
"#;
    let results = extract("orders.xsd", anonymous);

    assert!(facts_with_pattern(&results, "xml.xsd.type.v1").is_empty());
    assert_eq!(facts_with_pattern(&results, "xml.xsd.element.v1").len(), 1);
}

#[test]
fn repeated_anonymous_elements_do_not_multiply_facts() {
    let mut source = String::from("<catalog name=\"parts\">\n  <rows>\n");
    for index in 0..500 {
        source.push_str(&format!(
            "    <row><cell>{index:05}</cell><cell>bolt-{index:05}</cell></row>\n"
        ));
    }
    source.push_str("  </rows>\n</catalog>\n");

    let results = extract("catalog.xml", &source);

    assert_eq!(pattern_ids(&results), BTreeSet::from(["xml.document.v1"]));
    assert_eq!(
        metadata_u64(only_fact(&results, "xml.document.v1"), "element_count"),
        Some(1502)
    );
}

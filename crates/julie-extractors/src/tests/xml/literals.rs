//! XML attribute-value literal capture.
//!
//! Attribute values are the configuration payload of an XML document, so every
//! non-empty value is recorded under the shared markup `tag.attribute` carrier.

use super::support::parse;
use crate::base::{Literal, LiteralKind};
use crate::xml::XmlExtractor;
use std::path::PathBuf;

fn capture(code: &str) -> Vec<Literal> {
    let tree = parse(code);
    let mut extractor = XmlExtractor::new(
        "xml".to_string(),
        "schema.xml".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
    extractor.base.take_literals()
}

fn carrier_of<'a>(literals: &'a [Literal], text: &str) -> Option<&'a str> {
    literals
        .iter()
        .find(|literal| literal.literal_text == text)
        .and_then(|literal| literal.carrier.as_deref())
}

#[test]
fn attribute_values_carry_tag_and_attribute_carriers() {
    let code = r#"<configuration name="phonebook">
  <sink name="console" type="Serilog.Sinks.Console" />
  <add id="Timeout">30</add>
  <add />
</configuration>
"#;

    let literals = capture(code);

    assert_eq!(
        carrier_of(&literals, "phonebook"),
        Some("configuration.name")
    );
    assert_eq!(carrier_of(&literals, "console"), Some("sink.name"));
    assert_eq!(
        carrier_of(&literals, "Serilog.Sinks.Console"),
        Some("sink.type")
    );
    assert_eq!(carrier_of(&literals, "Timeout"), Some("add.id"));
    assert_eq!(literals.len(), 4, "an attribute-less tag emits nothing");
}

#[test]
fn prefixed_tags_and_attributes_keep_their_qname_in_the_carrier() {
    let code = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Phone" type="xs:string" />
</xs:schema>
"#;

    let literals = capture(code);

    assert_eq!(
        carrier_of(&literals, "http://www.w3.org/2001/XMLSchema"),
        Some("xs:schema.xmlns:xs")
    );
    assert_eq!(carrier_of(&literals, "Phone"), Some("xs:element.name"));
    assert_eq!(carrier_of(&literals, "xs:string"), Some("xs:element.type"));
}

#[test]
fn attribute_literals_anchor_to_the_enclosing_named_element() {
    let code = r#"<configuration name="phonebook">
  <sink name="console" type="Serilog.Sinks.Console" />
</configuration>
"#;

    let tree = parse(code);
    let mut extractor = XmlExtractor::new(
        "xml".to_string(),
        "schema.xml".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
    let literals = extractor.base.take_literals();

    let console = symbols
        .iter()
        .find(|symbol| symbol.name == "console")
        .expect("expected console symbol");
    let literal = literals
        .iter()
        .find(|literal| literal.literal_text == "Serilog.Sinks.Console")
        .expect("expected sink type literal");

    assert_eq!(
        literal.containing_symbol_id.as_deref(),
        Some(console.id.as_str())
    );
    assert_eq!(literal.kind, LiteralKind::Other);
}

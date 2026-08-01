use std::collections::HashSet;

use tree_sitter::Node;

use super::elements;
use crate::base::config_literals::tag_attribute_carrier;
use crate::base::{BaseExtractor, IdentifierKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Attributes whose value names another schema component. The value is recorded
/// exactly as written (`tns:AddPhone`); v1 performs no namespace resolution.
const REFERENCE_ATTRIBUTES: [&str; 4] = ["base", "element", "ref", "type"];

/// Carrier tag for an element the grammar left unnamed.
const UNNAMED_TAG: &str = "element";

/// Namespaces whose ELEMENTS declare schema or service components, so a
/// reference-named attribute they own really does name another component.
/// Written without the trailing slash WSDL 1.1 canonically carries, because
/// [`normalize`] strips it from both sides.
const COMPONENT_ELEMENT_NAMESPACES: [&str; 3] = [
    "http://www.w3.org/2001/XMLSchema",
    "http://schemas.xmlsoap.org/wsdl",
    "http://www.w3.org/ns/wsdl",
];

/// Namespaces whose ATTRIBUTES name a schema component wherever they appear.
/// `xsi:type` on an instance document declares that element's type even though
/// the element itself is not a schema component.
const COMPONENT_ATTRIBUTE_NAMESPACES: [&str; 2] = [
    "http://www.w3.org/2001/XMLSchema",
    "http://www.w3.org/2001/XMLSchema-instance",
];

/// The document's `xmlns` bindings, reduced to the question the reference tier
/// asks of them: is this name in a namespace that makes it a schema reference?
///
/// Bindings are collected document-wide rather than per element. Redeclaring a
/// prefix to a different URI part-way down a document is vanishingly rare, and
/// the alternative — threading a scope through the reference walk — buys nothing
/// a real schema, WSDL, or config document would notice.
#[derive(Default)]
pub(super) struct SchemaNamespaces {
    /// Prefixes bound to a namespace whose elements are schema components.
    element_prefixes: HashSet<String>,
    /// Whether the default namespace makes unprefixed elements components.
    default_is_component: bool,
    /// Prefixes bound to a namespace whose attributes name components.
    attribute_prefixes: HashSet<String>,
}

impl SchemaNamespaces {
    pub(super) fn scan(base: &BaseExtractor, root: Node<'_>) -> Self {
        let mut namespaces = Self::default();
        namespaces.collect(base, root, 0);
        namespaces
    }

    fn collect(&mut self, base: &BaseExtractor, node: Node<'_>, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        if let Some(tag) = tag_of(node) {
            for (name, value_node) in elements::attributes(base, tag) {
                let uri = normalize(&elements::attribute_value(base, value_node));

                if name == "xmlns" {
                    self.default_is_component |=
                        COMPONENT_ELEMENT_NAMESPACES.contains(&uri.as_str());
                    continue;
                }

                let Some(prefix) = name.strip_prefix("xmlns:") else {
                    continue;
                };

                if COMPONENT_ELEMENT_NAMESPACES.contains(&uri.as_str()) {
                    self.element_prefixes.insert(prefix.to_string());
                }
                if COMPONENT_ATTRIBUTE_NAMESPACES.contains(&uri.as_str()) {
                    self.attribute_prefixes.insert(prefix.to_string());
                }
            }
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect(base, child, child_depth);
        }
    }

    /// Whether a reference-named attribute on this element names a component.
    fn qualifies(&self, tag_name: &str, attribute_name: &str) -> bool {
        self.element_is_component(tag_name) || self.attribute_is_component(attribute_name)
    }

    fn element_is_component(&self, tag_name: &str) -> bool {
        match tag_name.split_once(':') {
            Some((prefix, _)) => self.element_prefixes.contains(prefix),
            None => self.default_is_component,
        }
    }

    /// An unprefixed attribute is in no namespace at all, so only a prefixed one
    /// can qualify on its own.
    fn attribute_is_component(&self, attribute_name: &str) -> bool {
        match attribute_name.split_once(':') {
            Some((prefix, _)) => self.attribute_prefixes.contains(prefix),
            None => false,
        }
    }
}

fn tag_of(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "element" {
        return elements::tag_node(node);
    }
    matches!(node.kind(), "STag" | "EmptyElemTag").then_some(node)
}

/// WSDL is quoted both with and without its trailing slash in the wild.
fn normalize(uri: &str) -> String {
    uri.trim().trim_end_matches('/').to_string()
}

/// Every non-empty attribute value becomes a literal, and the subset naming
/// another schema component also becomes a `type_usage` identifier.
///
/// Attribute values are where an XML document keeps its configuration payload,
/// so they are captured under the same `tag.attribute` carrier html and vue
/// use. The tag half keeps its prefix and case because XML names are
/// case-sensitive; [`tag_attribute_carrier`] lowercases the attribute half for
/// cross-markup matching.
///
/// A reference-named attribute only becomes an identifier in schema context:
/// `type`, `ref`, `base`, and `element` are ordinary words, and `<button
/// type="button">` in a generic document names no component.
pub(super) fn extract_element_facts(
    base: &mut BaseExtractor,
    tag: Node<'_>,
    namespaces: &SchemaNamespaces,
    containing_symbol_id: Option<&str>,
) {
    let tag_name = elements::tag_name(base, tag).unwrap_or_else(|| UNNAMED_TAG.to_string());

    for (name, value_node) in elements::attributes(base, tag) {
        let value = elements::attribute_value(base, value_node);
        if value.trim().is_empty() {
            continue;
        }

        base.record_literal(
            &value_node,
            value.clone(),
            Some(tag_attribute_carrier(&tag_name, &name)),
            0,
            containing_symbol_id.map(str::to_string),
        );

        if !REFERENCE_ATTRIBUTES.contains(&elements::local_name(&name))
            || !namespaces.qualifies(&tag_name, &name)
        {
            continue;
        }

        base.create_identifier(
            &value_node,
            value,
            IdentifierKind::TypeUsage,
            containing_symbol_id.map(str::to_string),
        );
    }
}

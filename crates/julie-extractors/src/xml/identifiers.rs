use tree_sitter::Node;

use super::elements;
use crate::base::config_literals::tag_attribute_carrier;
use crate::base::{BaseExtractor, IdentifierKind};

/// Attributes whose value names another schema component. The value is recorded
/// exactly as written (`tns:AddPhone`); v1 performs no namespace resolution.
const REFERENCE_ATTRIBUTES: [&str; 4] = ["base", "element", "ref", "type"];

/// Carrier tag for an element the grammar left unnamed.
const UNNAMED_TAG: &str = "element";

/// Every non-empty attribute value becomes a literal, and the subset naming
/// another component also becomes a `type_usage` identifier.
///
/// Attribute values are where an XML document keeps its configuration payload,
/// so they are captured under the same `tag.attribute` carrier html and vue
/// use. The tag half keeps its prefix and case because XML names are
/// case-sensitive; [`tag_attribute_carrier`] lowercases the attribute half for
/// cross-markup matching.
pub(super) fn extract_element_facts(
    base: &mut BaseExtractor,
    tag: Node<'_>,
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

        if !REFERENCE_ATTRIBUTES.contains(&elements::local_name(&name)) {
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

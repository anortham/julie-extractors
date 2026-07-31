use tree_sitter::Node;

use super::elements;
use crate::base::{BaseExtractor, IdentifierKind};

/// Attributes whose value names another schema component. The value is recorded
/// exactly as written (`tns:AddPhone`); v1 performs no namespace resolution.
const REFERENCE_ATTRIBUTES: [&str; 4] = ["base", "element", "ref", "type"];

pub(super) fn extract_element_references(
    base: &mut BaseExtractor,
    tag: Node<'_>,
    containing_symbol_id: Option<&str>,
) {
    for (name, value_node) in elements::attributes(base, tag) {
        if !REFERENCE_ATTRIBUTES.contains(&elements::local_name(&name)) {
            continue;
        }

        let value = elements::attribute_value(base, value_node);
        if value.trim().is_empty() {
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

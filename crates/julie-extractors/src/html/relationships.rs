use crate::base::relationship_resolution::{StructuredPendingRelationship, UnresolvedTarget};
use crate::base::{BaseExtractor, RelationshipKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

use super::helpers::HTMLHelpers;

/// Relationship extraction logic for HTML elements
pub(super) struct RelationshipExtractor;

impl RelationshipExtractor {
    /// Phase 4b.html — walk the tree and emit StructuredPendingRelationship
    /// for external `<script src=...>` and `<link href=...>` references.
    pub(super) fn collect_structured_pending(
        base: &BaseExtractor,
        node: Node,
        symbols: &[Symbol],
        pending: &mut Vec<StructuredPendingRelationship>,
    ) {
        Self::collect_structured_pending_at_depth(base, node, symbols, pending, 0);
    }

    fn collect_structured_pending_at_depth(
        base: &BaseExtractor,
        node: Node,
        symbols: &[Symbol],
        pending: &mut Vec<StructuredPendingRelationship>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        match node.kind() {
            "script_element" => {
                Self::emit_resource_pending(
                    base,
                    node,
                    symbols,
                    ("src", "html-script-src", RelationshipKind::Imports),
                    pending,
                );
            }
            "element" => {
                if let Some(link) = HTMLHelpers::extract_tag_name(base, node)
                    .and_then(|tag| resource_attribute(base, node, &tag))
                {
                    Self::emit_resource_pending(base, node, symbols, link, pending);
                }
            }
            _ => {}
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::collect_structured_pending_at_depth(base, child, symbols, pending, child_depth);
        }
    }

    fn emit_resource_pending(
        base: &BaseExtractor,
        node: Node,
        symbols: &[Symbol],
        (attribute, import_context, kind): ResourceAttribute,
        pending: &mut Vec<StructuredPendingRelationship>,
    ) {
        let attributes = HTMLHelpers::extract_attributes(base, node);
        let Some(value) = attributes.get(attribute).map(|value| value.trim()) else {
            return;
        };
        if value.is_empty() || is_templated(value) {
            return;
        }
        if kind != RelationshipKind::Imports && !is_document_target(value) {
            return;
        }
        let line_number = (node.start_position().row + 1) as u32;
        let caller_id = Self::find_element_symbol(base, node, symbols)
            .map(|symbol| symbol.id.clone())
            .unwrap_or_else(|| format!("file:{}", base.file_path));
        let mut target = UnresolvedTarget::simple(value.to_string());
        target.import_context = Some(import_context.to_string());
        pending.push(StructuredPendingRelationship::new(
            caller_id.clone(),
            target,
            Some(caller_id),
            kind,
            base.file_path.clone(),
            line_number,
            0.9,
        ));
    }

    /// The symbol built from this element node.
    pub(super) fn find_element_symbol<'a>(
        base: &BaseExtractor,
        node: Node,
        symbols: &'a [Symbol],
    ) -> Option<&'a Symbol> {
        symbols.iter().find(|symbol| {
            symbol.start_byte as usize == node.start_byte() && symbol.file_path == base.file_path
        })
    }
}

/// The attribute that names another document or resource, its import
/// context, and the relationship it expresses.
type ResourceAttribute = (&'static str, &'static str, RelationshipKind);

fn resource_attribute(base: &BaseExtractor, node: Node, tag: &str) -> Option<ResourceAttribute> {
    Some(match tag {
        "link" if is_resource_link(base, node) => {
            ("href", "html-link-href", RelationshipKind::Imports)
        }
        "script" => ("src", "html-script-src", RelationshipKind::Imports),
        "a" => ("href", "html-anchor-href", RelationshipKind::References),
        "area" => ("href", "html-area-href", RelationshipKind::References),
        "iframe" => ("src", "html-iframe-src", RelationshipKind::Uses),
        "embed" => ("src", "html-embed-src", RelationshipKind::Uses),
        "object" => ("data", "html-object-data", RelationshipKind::Uses),
        "form" => ("action", "html-form-action", RelationshipKind::Calls),
        _ => return None,
    })
}

/// `<link rel>` values that load a resource. `canonical`, `alternate`,
/// `preconnect`, and `dns-prefetch` only describe or warm up a URL.
const RESOURCE_LINK_RELS: &[&str] = &[
    "stylesheet",
    "modulepreload",
    "preload",
    "prefetch",
    "icon",
    "apple-touch-icon",
    "apple-touch-icon-precomposed",
    "mask-icon",
    "manifest",
    "import",
];

fn is_resource_link(base: &BaseExtractor, node: Node) -> bool {
    HTMLHelpers::extract_attributes(base, node)
        .get("rel")
        .is_some_and(|rel| {
            rel.split_ascii_whitespace()
                .any(|token| RESOURCE_LINK_RELS.contains(&token.to_ascii_lowercase().as_str()))
        })
}

/// A target in this site: not an in-page fragment, not another scheme
/// (`mailto:`, `https:`), and not a protocol-relative URL.
fn is_document_target(value: &str) -> bool {
    let has_scheme = value.split_once(':').is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    !value.starts_with('#') && !value.starts_with("//") && !has_scheme
}

/// A value that a server-side template fills in (`{{ }}`, `{% %}`, `<% %>`).
pub(super) fn is_templated(value: &str) -> bool {
    ["{{", "{%", "<%", "{#"]
        .iter()
        .any(|open| value.contains(open))
}

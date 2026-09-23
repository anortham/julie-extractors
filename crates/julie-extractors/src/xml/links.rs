//! Links from an XML document to other files: `<?xml-stylesheet href?>`,
//! `<?xml-model href?>`, the DOCTYPE system id, external entities, XInclude
//! `href`, `xsi:schemaLocation` and `xsi:noNamespaceSchemaLocation`, and XSLT
//! `xsl:import`/`xsl:include`.

use tree_sitter::{Node, Tree};

use super::context::{XINCLUDE_NS, XSI_NS, XSLT_NS, element_namespace, resolve_prefix, tag_of};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(crate) struct DocumentLink<'tree> {
    pub node: Node<'tree>,
    pub href: String,
    pub kind: &'static str,
    pub namespace: Option<String>,
}

impl DocumentLink<'_> {
    /// Whether the link names a local file a consumer can resolve.
    pub(crate) fn imports_file(&self) -> bool {
        !self.href.is_empty() && !self.href.contains("://") && !self.href.starts_with('#')
    }
}

pub(crate) fn document_links<'tree>(content: &str, tree: &'tree Tree) -> Vec<DocumentLink<'tree>> {
    let mut links = Vec::new();
    collect(content, tree.root_node(), &mut links, 0);
    links
}

fn collect<'tree>(
    content: &str,
    node: Node<'tree>,
    links: &mut Vec<DocumentLink<'tree>>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "StyleSheetPI" | "XmlModelPI" => {
            if let Some(href) = pseudo_attribute(content, node, "href") {
                let kind = if node.kind() == "StyleSheetPI" {
                    "stylesheet"
                } else {
                    "xml_model"
                };
                links.push(link(node, href, kind, None));
            }
        }
        "doctypedecl" | "GEDecl" => {
            let mut cursor = node.walk();
            let system = node
                .children(&mut cursor)
                .find(|child| child.kind() == "ExternalID")
                .and_then(|external| system_uri(content, external));
            if let Some(href) = system {
                let kind = if node.kind() == "doctypedecl" {
                    "dtd"
                } else {
                    "external_entity"
                };
                links.push(link(node, href, kind, None));
            }
        }
        "STag" | "EmptyElemTag" => element_links(content, node, links),
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(content, child, links, child_depth);
    }
}

fn link<'tree>(
    node: Node<'tree>,
    href: String,
    kind: &'static str,
    namespace: Option<String>,
) -> DocumentLink<'tree> {
    DocumentLink {
        node,
        href,
        kind,
        namespace,
    }
}

fn element_links<'tree>(content: &str, tag: Node<'tree>, links: &mut Vec<DocumentLink<'tree>>) {
    let node = tag
        .parent()
        .filter(|parent| parent.kind() == "element")
        .unwrap_or(tag);
    let Some(tag) = tag_of(node) else {
        return;
    };
    let namespace = element_namespace(content, tag);
    let tag_local = super::context::tag_name_text(content, tag)
        .map(|name| name.rsplit(':').next().unwrap_or(name));
    let attributes = attributes(content, tag);
    let attribute = |wanted: &str| {
        attributes
            .iter()
            .find(|(name, _)| name == wanted)
            .map(|(_, value)| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    match (namespace.as_deref(), tag_local) {
        (Some(XINCLUDE_NS), Some("include")) => {
            if let Some(href) = attribute("href") {
                links.push(link(node, href, "xinclude", None));
            }
        }
        (Some(XSLT_NS), Some(local @ ("import" | "include"))) => {
            if let Some(href) = attribute("href") {
                let kind = if local == "import" {
                    "xsl_import"
                } else {
                    "xsl_include"
                };
                links.push(link(node, href, kind, None));
            }
        }
        _ => {}
    }
    for (name, value) in &attributes {
        let Some((prefix, local)) = name.split_once(':') else {
            continue;
        };
        if resolve_prefix(content, tag, Some(prefix)).as_deref() != Some(XSI_NS) {
            continue;
        }
        match local {
            "schemaLocation" => {
                let parts: Vec<&str> = value.split_whitespace().collect();
                for pair in parts.chunks(2) {
                    if let [namespace, location] = pair {
                        links.push(link(
                            node,
                            location.to_string(),
                            "schema_location",
                            Some(namespace.to_string()),
                        ));
                    }
                }
            }
            "noNamespaceSchemaLocation" if !value.trim().is_empty() => {
                links.push(link(
                    node,
                    value.trim().to_string(),
                    "no_namespace_schema_location",
                    None,
                ));
            }
            _ => {}
        }
    }
}

fn attributes(content: &str, tag: Node<'_>) -> Vec<(String, String)> {
    let mut cursor = tag.walk();
    tag.children(&mut cursor)
        .filter(|child| child.kind() == "Attribute")
        .filter_map(|attribute| {
            let mut parts = attribute.walk();
            let mut name = None;
            let mut value = None;
            for part in attribute.children(&mut parts) {
                match part.kind() {
                    "Name" if name.is_none() => name = content.get(part.byte_range()),
                    "AttValue" => value = content.get(part.byte_range()),
                    _ => {}
                }
            }
            let value = value?;
            Some((
                name?.to_string(),
                value.get(1..value.len().saturating_sub(1))?.to_string(),
            ))
        })
        .collect()
}

fn pseudo_attribute(content: &str, pi: Node<'_>, wanted: &str) -> Option<String> {
    let mut cursor = pi.walk();
    pi.children(&mut cursor)
        .filter(|child| child.kind() == "PseudoAtt")
        .find_map(|attribute| {
            let mut parts = attribute.walk();
            let children: Vec<Node> = attribute.children(&mut parts).collect();
            let name = children.iter().find(|part| part.kind() == "Name")?;
            if content.get(name.byte_range())? != wanted {
                return None;
            }
            let value = children
                .iter()
                .find(|part| part.kind() == "PseudoAttValue")?;
            let text = content.get(value.byte_range())?;
            let text = text.get(1..text.len().saturating_sub(1))?.trim();
            (!text.is_empty()).then(|| text.to_string())
        })
}

/// The URI of an `ExternalID`'s `SystemLiteral`.
pub(crate) fn system_uri(content: &str, external: Node<'_>) -> Option<String> {
    let mut cursor = external.walk();
    let literal = external
        .children(&mut cursor)
        .find(|child| child.kind() == "SystemLiteral")?;
    let text = content.get(literal.byte_range())?;
    let text = text.get(1..text.len().saturating_sub(1))?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Node;

use super::build::{self, BuildDialect};
use super::context::{
    ANDROID_NS, Framework, WSDL11_NS, WSDL20_NS, XAML_NS, XSD_NS, XSLT_NS, XmlContext,
    element_namespace, resolve_prefix,
};
use crate::base::{
    BaseExtractor, NormalizedSpan, Symbol, SymbolKind, SymbolOptions, TestRole, Visibility,
};
use crate::test_detection::apply_test_role;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Attributes that promote an element to a symbol, in priority order.
const NAME_ATTRIBUTES: [&str; 2] = ["name", "id"];

const MAX_DOC_CHARS: usize = 2000;

/// XSLT instructions whose `name` builds output or passes an argument; they
/// declare nothing.
const XSLT_NON_DECLARATIONS: &[&str] = &[
    "call-template",
    "with-param",
    "attribute",
    "element",
    "processing-instruction",
    "namespace",
];

/// Android manifest elements whose `android:name` names a class, a platform
/// permission, or an intent constant rather than declaring anything.
const ANDROID_REFERENCE_ELEMENTS: &[&str] = &[
    "application",
    "activity",
    "service",
    "receiver",
    "provider",
    "uses-permission",
    "uses-permission-sdk-23",
    "uses-feature",
    "uses-library",
    "uses-sdk",
    "action",
    "category",
];

/// The start tag of an element: `STag` for `<a>…</a>`, `EmptyElemTag` for `<a/>`.
pub(super) fn tag_node<'tree>(element: Node<'tree>) -> Option<Node<'tree>> {
    let mut cursor = element.walk();
    element
        .children(&mut cursor)
        .find(|child| matches!(child.kind(), "STag" | "EmptyElemTag"))
}

pub(super) fn tag_name(base: &BaseExtractor, tag: Node<'_>) -> Option<String> {
    let mut cursor = tag.walk();
    tag.children(&mut cursor)
        .find(|child| child.kind() == "Name")
        .map(|name| base.get_node_text(&name))
}

/// Attribute name paired with its `AttValue` node, in source order.
pub(super) fn attributes<'tree>(
    base: &BaseExtractor,
    tag: Node<'tree>,
) -> Vec<(String, Node<'tree>)> {
    let mut cursor = tag.walk();
    let mut attributes = Vec::new();

    for child in tag.children(&mut cursor) {
        if child.kind() != "Attribute" {
            continue;
        }

        let mut attribute_cursor = child.walk();
        let mut name = None;
        let mut value = None;
        for part in child.children(&mut attribute_cursor) {
            match part.kind() {
                "Name" if name.is_none() => name = Some(base.get_node_text(&part)),
                "AttValue" => value = Some(part),
                _ => {}
            }
        }

        if let (Some(name), Some(value)) = (name, value) {
            attributes.push((name, value));
        }
    }

    attributes
}

pub(super) fn attribute_value(base: &BaseExtractor, value: Node<'_>) -> String {
    let text = base.get_node_text(&value);
    let unquoted = text.strip_prefix(['"', '\'']).unwrap_or(&text);
    let unquoted = unquoted.strip_suffix(['"', '\'']).unwrap_or(unquoted);
    unquoted.to_string()
}

/// `xsi:type` and `type` name the same attribute; `tns:Address` names `Address`.
pub(super) fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// What an element declares: its name, where the name comes from, and the
/// symbol kind of the declared component.
struct Declaration {
    name_attribute: String,
    name: String,
    kind: SymbolKind,
    metadata: Vec<(&'static str, String)>,
}

pub(super) fn extract_element(
    base: &mut BaseExtractor,
    context: &XmlContext,
    element: Node<'_>,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let tag = tag_node(element)?;
    let declaration = declaration(base, context, element, tag)?;
    extract_from_tag(base, context, element, tag, parent_id, declaration)
}

/// An element whose name comes from somewhere other than its own name
/// attribute: a manifest's root (file name, package id, artifactId) or a
/// child element's text. It is a module: it groups the manifest's content.
pub(super) fn extract_named_element(
    base: &mut BaseExtractor,
    context: &XmlContext,
    element: Node<'_>,
    parent_id: Option<&str>,
    name_attribute: &str,
    name: String,
    extra_metadata: Vec<(&'static str, String)>,
) -> Option<Symbol> {
    let tag = tag_node(element)?;
    extract_from_tag(
        base,
        context,
        element,
        tag,
        parent_id,
        Declaration {
            name_attribute: name_attribute.to_string(),
            name,
            kind: SymbolKind::Module,
            metadata: extra_metadata,
        },
    )
}

/// A start tag stranded in an ERROR region — an unclosed element — still names a
/// component. Recovering it keeps one missing end tag from collapsing the whole
/// document to zero symbols.
pub(super) fn extract_orphan_tag(
    base: &mut BaseExtractor,
    context: &XmlContext,
    tag: Node<'_>,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let declaration = declaration(base, context, tag, tag)?;
    extract_from_tag(base, context, tag, tag, parent_id, declaration)
}

/// Internal DTD declarations: `<!ELEMENT>` declares an element type and
/// `<!ENTITY>` a general entity.
pub(super) fn extract_dtd_declaration(
    base: &mut BaseExtractor,
    node: Node<'_>,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let (kind, tag) = match node.kind() {
        "elementdecl" => (SymbolKind::Type, "!ELEMENT"),
        "GEDecl" => (SymbolKind::Constant, "!ENTITY"),
        _ => return None,
    };
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let name = children
        .iter()
        .find(|child| child.kind() == "Name")
        .map(|name| base.get_node_text(name))?;
    let mut metadata = HashMap::from([
        ("tag".to_string(), Value::String(tag.to_string())),
        (
            "name_attribute".to_string(),
            Value::String("Name".to_string()),
        ),
    ]);
    for child in &children {
        match child.kind() {
            "EntityValue" => {
                let value = base.get_node_text(child);
                let value = value.get(1..value.len().saturating_sub(1)).unwrap_or("");
                metadata.insert("value".to_string(), Value::String(value.to_string()));
            }
            "ExternalID" => {
                if let Some(system) = super::links::system_uri(&base.content, *child) {
                    metadata.insert("system_id".to_string(), Value::String(system));
                }
            }
            _ => {}
        }
    }
    let content = base.content.clone();
    let doc_comment = leading_comment_doc(&content, node);
    let signature = collapse_whitespace(&base.get_node_text(&node));
    Some(base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

fn extract_from_tag(
    base: &mut BaseExtractor,
    context: &XmlContext,
    span_node: Node<'_>,
    tag: Node<'_>,
    parent_id: Option<&str>,
    declaration: Declaration,
) -> Option<Symbol> {
    let tag_name = tag_name(base, tag)?;
    let signature = collapse_whitespace(&base.get_node_text(&tag));
    let content = base.content.clone();
    let doc_comment = documentation(&content, context, span_node, &tag_name);

    let role = test_role(
        base,
        context,
        span_node,
        &tag_name,
        &declaration.name_attribute,
    );
    let mut metadata = HashMap::new();
    metadata.insert("tag".to_string(), Value::String(tag_name));
    metadata.insert(
        "name_attribute".to_string(),
        Value::String(declaration.name_attribute),
    );
    for (key, value) in declaration.metadata {
        metadata.insert(key.to_string(), Value::String(value));
    }
    if let Some(role) = role {
        apply_test_role(&mut metadata, role);
    }

    let mut symbol = base.create_symbol(
        &span_node,
        declaration.name,
        declaration.kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    );
    let body_span = element_content(span_node).map(|content| NormalizedSpan::from_node(&content));
    base.set_body_span(&mut symbol, body_span);
    Some(symbol)
}

fn declaration(
    base: &BaseExtractor,
    context: &XmlContext,
    span_node: Node<'_>,
    tag: Node<'_>,
) -> Option<Declaration> {
    let content = base.content.as_str();
    let tag_name = tag_name(base, tag)?;
    let local = local_name(&tag_name);
    let namespace = element_namespace(content, tag);
    let namespace = namespace.as_deref();
    let is_root = is_document_root(span_node);
    let attributes = attributes(base, tag);
    let attribute = |wanted: &str| {
        attributes
            .iter()
            .find(|(name, _)| name == wanted)
            .map(|(_, value)| attribute_value(base, *value))
            .filter(|value| !value.trim().is_empty())
    };

    if is_skipped(content, context, span_node, local, namespace) {
        return None;
    }

    let special = match (context.framework, local) {
        (Framework::MyBatis, "mapper") if is_root => {
            attribute("namespace").map(|namespace| ("namespace", namespace, SymbolKind::Module))
        }
        (Framework::WebApp, "servlet" | "filter") if span_node.kind() == "element" => {
            let field = if local == "servlet" {
                "servlet-name"
            } else {
                "filter-name"
            };
            build::child_text(content, span_node, field)
                .map(|name| (field, name, SymbolKind::Variable))
        }
        _ => None,
    };
    if let Some((source, name, kind)) = special {
        return Some(Declaration {
            name_attribute: source.to_string(),
            name,
            kind,
            metadata: Vec::new(),
        });
    }
    if is_root && let Some(class) = xaml_class(base, &attributes, tag) {
        let simple = class.rsplit('.').next().unwrap_or(&class).to_string();
        return Some(Declaration {
            name_attribute: "Class".to_string(),
            name: simple,
            kind: SymbolKind::Class,
            metadata: vec![("qualified_name", class)],
        });
    }
    if namespace == Some(XSD_NS)
        && local == "enumeration"
        && let Some(value) = attribute("value")
    {
        return Some(Declaration {
            name_attribute: "value".to_string(),
            name: value,
            kind: SymbolKind::EnumMember,
            metadata: Vec::new(),
        });
    }
    if local == "add"
        && promoted_name(base, context, tag, &attributes).is_none()
        && let Some(key) = attribute("key")
    {
        return Some(Declaration {
            name_attribute: "key".to_string(),
            name: key,
            kind: SymbolKind::Variable,
            metadata: attribute("value")
                .map(|value| ("value", value))
                .into_iter()
                .collect(),
        });
    }

    let (name_attribute, name) = promoted_name(base, context, tag, &attributes)?;
    let has_children = span_node.kind() == "element" && has_child_element(span_node);
    let kind = declared_kind(base, context, span_node, local, namespace, &name_attribute)
        .unwrap_or(if has_children {
            SymbolKind::Module
        } else {
            SymbolKind::Variable
        });
    let mut metadata = Vec::new();
    if context.framework == Framework::Resx && local == "data" && span_node.kind() == "element" {
        if let Some(value) = build::child_text(content, span_node, "value") {
            metadata.push(("value", value));
        }
        for (key, attribute_name) in [("resource_type", "type"), ("mimetype", "mimetype")] {
            if let Some(value) = attribute(attribute_name) {
                metadata.push((key, value));
            }
        }
    }
    Some(Declaration {
        name_attribute,
        name,
        kind,
        metadata,
    })
}

fn is_skipped(
    content: &str,
    context: &XmlContext,
    span_node: Node<'_>,
    local: &str,
    namespace: Option<&str>,
) -> bool {
    if namespace == Some(XSLT_NS) && XSLT_NON_DECLARATIONS.contains(&local) {
        return true;
    }
    match context.framework {
        Framework::Resx => {
            local == "resheader"
                || std::iter::successors(Some(span_node), |node| node.parent())
                    .filter(|node| node.kind() == "element")
                    .any(|element| build::local_tag(content, element) == Some("schema"))
        }
        Framework::AndroidManifest => ANDROID_REFERENCE_ELEMENTS.contains(&local),
        Framework::TestNg => matches!(local, "class" | "include" | "exclude" | "package"),
        _ => false,
    }
}

/// The kind of the component an element declares, when its vocabulary is known.
fn declared_kind(
    base: &BaseExtractor,
    context: &XmlContext,
    span_node: Node<'_>,
    local: &str,
    namespace: Option<&str>,
    name_attribute: &str,
) -> Option<SymbolKind> {
    let content = base.content.as_str();
    let kind = match namespace {
        Some(XSD_NS) => match local {
            "complexType" => SymbolKind::Class,
            "simpleType" if is_enumeration(content, span_node) => SymbolKind::Enum,
            "simpleType" => SymbolKind::Type,
            "element" => SymbolKind::Field,
            "attribute" => SymbolKind::Property,
            "group" | "attributeGroup" => SymbolKind::Type,
            "key" | "unique" | "keyref" | "notation" => SymbolKind::Constant,
            "schema" => SymbolKind::Module,
            _ => return None,
        },
        Some(WSDL11_NS | WSDL20_NS) => match local {
            "definitions" | "description" | "service" => SymbolKind::Module,
            "message" => SymbolKind::Struct,
            "part" | "input" | "output" | "fault" | "infault" | "outfault" => SymbolKind::Field,
            "portType" | "interface" => SymbolKind::Interface,
            "operation" => SymbolKind::Method,
            "binding" => SymbolKind::Class,
            "port" | "endpoint" => SymbolKind::Property,
            _ => return None,
        },
        Some(XSLT_NS) => match local {
            "template" | "function" => SymbolKind::Function,
            "param" | "variable" => SymbolKind::Variable,
            "key" => SymbolKind::Constant,
            _ => return None,
        },
        _ => {
            if context.android && name_attribute == "id" {
                return Some(SymbolKind::Field);
            }
            if name_attribute == "Name" && has_xaml_scope(content, span_node) {
                return Some(SymbolKind::Field);
            }
            match (context.framework, context.build, local) {
                (_, Some(BuildDialect::MsBuild), "Target") => SymbolKind::Function,
                (_, Some(BuildDialect::MsBuild), "UsingTask") => SymbolKind::Class,
                (Framework::Ant, _, "target" | "macrodef" | "scriptdef") => SymbolKind::Function,
                (Framework::Ant, _, "project") if is_document_root(span_node) => SymbolKind::Module,
                (Framework::Ant, _, "property") => SymbolKind::Variable,
                (Framework::Spring, _, "bean") => SymbolKind::Variable,
                (Framework::Spring, _, "property") => SymbolKind::Property,
                (Framework::MyBatis, _, "select" | "insert" | "update" | "delete") => {
                    SymbolKind::Method
                }
                (Framework::MyBatis, _, "sql") => SymbolKind::Function,
                (Framework::MyBatis, _, "resultMap" | "parameterMap") => SymbolKind::Struct,
                (Framework::TestNg, _, "suite" | "test") => SymbolKind::Module,
                (Framework::Resx, _, "data") => SymbolKind::Constant,
                _ => return None,
            }
        }
    };
    Some(kind)
}

fn is_enumeration(content: &str, simple_type: Node<'_>) -> bool {
    simple_type.kind() == "element"
        && build::child_elements(simple_type)
            .into_iter()
            .filter(|child| build::local_tag(content, *child) == Some("restriction"))
            .flat_map(build::child_elements)
            .any(|facet| build::local_tag(content, facet) == Some("enumeration"))
}

fn has_xaml_scope(content: &str, node: Node<'_>) -> bool {
    std::iter::successors(Some(node), |node| node.parent())
        .filter(|node| node.kind() == "element")
        .any(|element| {
            super::context::declared_namespaces(content, element)
                .iter()
                .any(|(_, uri)| uri == XAML_NS)
        })
}

/// The `x:Class` of a XAML root: the code-behind class the markup completes.
fn xaml_class(
    base: &BaseExtractor,
    attributes: &[(String, Node<'_>)],
    tag: Node<'_>,
) -> Option<String> {
    attributes.iter().find_map(|(name, value)| {
        let (prefix, local) = name.split_once(':')?;
        (local == "Class"
            && resolve_prefix(&base.content, tag, Some(prefix)).as_deref() == Some(XAML_NS))
        .then(|| attribute_value(base, *value))
        .filter(|class| !class.trim().is_empty())
    })
}

/// The documentation of a declaration: its own `xs:annotation/xs:documentation`
/// or WSDL `documentation` child, an Ant `description`, a `.resx` `comment`,
/// else the XML comments directly above it.
fn documentation(
    content: &str,
    context: &XmlContext,
    span_node: Node<'_>,
    tag_name: &str,
) -> Option<String> {
    let owned = if span_node.kind() == "element" {
        let children = build::child_elements(span_node);
        let annotation_docs = children
            .iter()
            .filter(|child| build::local_tag(content, **child) == Some("annotation"))
            .flat_map(|annotation| build::child_elements(*annotation))
            .chain(children.iter().copied())
            .filter(|child| build::local_tag(content, *child) == Some("documentation"))
            .filter_map(|documentation| element_text(content, documentation))
            .collect::<Vec<_>>();
        if !annotation_docs.is_empty() {
            Some(annotation_docs.join("\n"))
        } else if context.framework == Framework::Resx && local_name(tag_name) == "data" {
            build::child_text(content, span_node, "comment")
        } else {
            None
        }
    } else {
        None
    };
    let described = || {
        (context.framework == Framework::Ant)
            .then(|| build::attribute(content, span_node, "description"))
            .flatten()
            .map(|(_, description)| description)
    };
    owned
        .or_else(described)
        .or_else(|| leading_comment_doc(content, span_node))
        .map(|doc| doc.chars().take(MAX_DOC_CHARS).collect())
}

/// All character data under an element, one trimmed line per source line.
fn element_text(content: &str, element: Node<'_>) -> Option<String> {
    let mut text = String::new();
    collect_text(content, element, &mut text, 0);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn collect_text(content: &str, node: Node<'_>, out: &mut String, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(node.kind(), "CharData" | "CData") {
        out.push_str(content.get(node.byte_range()).unwrap_or(""));
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_text(content, child, out, child_depth);
    }
}

/// The XML comments directly above `node`, raw, with no blank line between
/// them or between the last one and the node.
pub(super) fn leading_comment_doc(content: &str, node: Node<'_>) -> Option<String> {
    let root = std::iter::successors(Some(node), |node| node.parent()).last()?;
    let mut comments = Vec::new();
    let mut end = node.start_byte();
    loop {
        let before = content.get(..end)?;
        let trimmed = before.trim_end();
        let gap = &before[trimmed.len()..];
        if gap.matches('\n').count() > 1 || !trimmed.ends_with("-->") {
            break;
        }
        let Some(comment) = root
            .descendant_for_byte_range(trimmed.len() - 1, trimmed.len())
            .and_then(|leaf| {
                std::iter::successors(Some(leaf), |node| node.parent())
                    .find(|node| node.kind() == "Comment")
            })
        else {
            break;
        };
        let line_start = content[..comment.start_byte()]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        comments.push(content.get(comment.byte_range())?.trim().to_string());
        if !content[line_start..comment.start_byte()].trim().is_empty() {
            break;
        }
        end = comment.start_byte();
    }
    comments.reverse();
    (!comments.is_empty()).then(|| comments.join("\n"))
}

/// True when `comment` is part of the leading doc block of the element or
/// declaration that follows it, by the rule of [`leading_comment_doc`].
pub(crate) fn comment_documents_following_element(content: &str, comment: Node<'_>) -> bool {
    let mut current = comment;
    loop {
        let rest = content.get(current.end_byte()..).unwrap_or("");
        let trimmed = rest.trim_start();
        if rest[..rest.len() - trimmed.len()].matches('\n').count() > 1 {
            return false;
        }
        let next_start = current.end_byte() + (rest.len() - trimmed.len());
        let root = std::iter::successors(Some(comment), |node| node.parent())
            .last()
            .unwrap_or(comment);
        let Some(next) = root
            .descendant_for_byte_range(next_start, next_start + 1)
            .and_then(|leaf| {
                std::iter::successors(Some(leaf), |node| node.parent()).find(|node| {
                    node.start_byte() == next_start
                        && matches!(
                            node.kind(),
                            "Comment"
                                | "element"
                                | "elementdecl"
                                | "GEDecl"
                                | "STag"
                                | "EmptyElemTag"
                        )
                })
            })
        else {
            return false;
        };
        if next.kind() != "Comment" {
            return true;
        }
        current = next;
    }
}

/// The `content` node between an element's start and end tags.
fn element_content(element: Node<'_>) -> Option<Node<'_>> {
    if element.kind() != "element" {
        return None;
    }
    let mut cursor = element.walk();
    element
        .children(&mut cursor)
        .find(|child| child.kind() == "content")
}

fn test_role(
    base: &BaseExtractor,
    context: &XmlContext,
    element: Node<'_>,
    tag_name: &str,
    name_attribute: &str,
) -> Option<TestRole> {
    if element.kind() != "element" {
        return None;
    }

    match tag_name {
        "target" if is_ant_target(base, element) => Some(TestRole::TestContainer),
        "test" if name_attribute == "name" && is_ant_test_case(base, element) => {
            Some(TestRole::TestCase)
        }
        "suite" if context.framework == Framework::TestNg && is_document_root(element) => {
            Some(TestRole::TestContainer)
        }
        "test"
            if context.framework == Framework::TestNg
                && nearest_element_parent(element)
                    .is_some_and(|parent| element_has_tag(base, parent, "suite")) =>
        {
            Some(TestRole::TestContainer)
        }
        _ => None,
    }
}

fn is_ant_target(base: &BaseExtractor, target: Node<'_>) -> bool {
    is_direct_child_of_ant_project(base, target) && contains_element_named(base, target, "junit", 0)
}

fn is_ant_test_case(base: &BaseExtractor, test: Node<'_>) -> bool {
    let Some(junit) = nearest_element_parent(test) else {
        return false;
    };
    if !element_has_tag(base, junit, "junit") {
        return false;
    }

    nearest_element_parent(junit).is_some_and(|target| is_ant_target(base, target))
}

fn is_direct_child_of_ant_project(base: &BaseExtractor, element: Node<'_>) -> bool {
    let Some(project) = nearest_element_parent(element) else {
        return false;
    };

    element_has_tag(base, project, "project") && is_document_root(project)
}

pub(super) fn is_document_root(element: Node<'_>) -> bool {
    let mut current = element.parent();
    while let Some(node) = current {
        if node.kind() == "document" {
            return node
                .child_by_field_name("root")
                .is_some_and(|root| root.start_byte() == element.start_byte());
        }
        current = node.parent();
    }

    false
}

fn nearest_element_parent(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "element" {
            return Some(parent);
        }
        current = parent.parent();
    }

    None
}

fn contains_element_named(base: &BaseExtractor, node: Node<'_>, wanted: &str, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "element" && element_has_tag(base, node, wanted) {
        return true;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| contains_element_named(base, child, wanted, child_depth))
}

fn element_has_tag(base: &BaseExtractor, element: Node<'_>, wanted: &str) -> bool {
    tag_node(element)
        .and_then(|tag| tag_name(base, tag))
        .is_some_and(|name| name == wanted)
}

pub(super) fn is_orphan_tag(node: Node<'_>) -> bool {
    matches!(node.kind(), "STag" | "EmptyElemTag")
        && node.parent().map(|parent| parent.kind()) != Some("element")
}

/// The first name attribute, matched by local name without case so MSBuild
/// `Name`, XAML `x:Name`, and `ID` count. `<UsingTask TaskName="…">` names a task.
///
/// In Android resources an id declares a name only as `@+id/name`, and
/// `android:name` names a class, so neither form of reference promotes.
fn promoted_name(
    base: &BaseExtractor,
    context: &XmlContext,
    tag: Node<'_>,
    attributes: &[(String, Node<'_>)],
) -> Option<(String, String)> {
    let using_task = tag_name(base, tag).is_some_and(|name| local_name(&name) == "UsingTask");
    let candidates: &[&str] = if using_task {
        &["TaskName"]
    } else {
        &NAME_ATTRIBUTES
    };
    for candidate in candidates {
        for (name, value) in attributes {
            if !local_name(name).eq_ignore_ascii_case(candidate) {
                continue;
            }
            let value = attribute_value(base, *value);
            if value.trim().is_empty() {
                continue;
            }
            let android_attribute = context.android
                && name.split_once(':').is_some_and(|(prefix, _)| {
                    resolve_prefix(&base.content, tag, Some(prefix)).as_deref() == Some(ANDROID_NS)
                });
            if android_attribute && *candidate == "id" {
                match value.strip_prefix("@+id/") {
                    Some(id) => return Some(("id".to_string(), id.to_string())),
                    None => continue,
                }
            }
            if android_attribute && context.framework != Framework::AndroidManifest {
                continue;
            }
            return Some((local_name(name).to_string(), value));
        }
    }

    None
}

fn has_child_element(element: Node<'_>) -> bool {
    let mut cursor = element.walk();
    element.children(&mut cursor).any(|child| {
        child.kind() == "content" && {
            let mut content_cursor = child.walk();
            child
                .children(&mut content_cursor)
                .any(|grandchild| grandchild.kind() == "element")
        }
    })
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

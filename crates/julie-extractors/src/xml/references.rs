//! Reference sites in XML documents: one list shared by the identifier and
//! relationship passes, so both agree on every site's span and owner.
//!
//! - XML Schema and WSDL QName attributes (`type`, `base`, `ref`, `element`,
//!   `itemType`, `memberTypes`, `substitutionGroup`, `refer`, `message`,
//!   `binding`, `interface`, and `xsi:type`) resolve their prefix through the
//!   in-scope `xmlns` declarations. A component declared in the same file under
//!   the matching `targetNamespace` is a same-file edge; any other namespace is
//!   a structured pending row whose import context is the `schemaLocation` of
//!   the matching `import`.
//! - Build targets (MSBuild and Ant), XSLT named templates, Spring beans,
//!   MyBatis result maps and SQL fragments, servlet names, and DTD entities
//!   resolve by name to declarations in the same file.
//! - Class names wired in Spring, Android, servlet, TestNG, and MyBatis files,
//!   and Android resource references, are identifiers only.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::build::{self, BuildDialect};
use super::context::{
    ANDROID_NS, ANDROID_TOOLS_NS, Framework, WSDL11_NS, WSDL20_NS, XSD_NS, XSI_NS, XSLT_NS,
    XmlContext, element_namespace, normalize_uri, resolve_prefix, tag_name_text,
};
use crate::base::{
    BaseExtractor, IdentifierKind, Relationship, RelationshipKind, StructuredPendingRelationship,
    Symbol, UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Namespaces whose elements declare schema or service components.
const COMPONENT_ELEMENT_NAMESPACES: [&str; 3] = [XSD_NS, WSDL11_NS, WSDL20_NS];

/// Namespaces whose attributes name a schema component wherever they appear.
const COMPONENT_ATTRIBUTE_NAMESPACES: [&str; 2] = [XSD_NS, XSI_NS];

const QNAME_ATTRIBUTES: &[&str] = &[
    "base",
    "element",
    "ref",
    "type",
    "itemType",
    "memberTypes",
    "substitutionGroup",
    "refer",
    "message",
    "binding",
    "interface",
];

const PREDEFINED_ENTITIES: &[&str] = &["lt", "gt", "amp", "apos", "quot"];

/// The component family a QName names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Category {
    Type,
    Element,
    Attribute,
    Group,
    AttributeGroup,
    Key,
    Message,
    PortType,
    Binding,
}

impl Category {
    fn declared_by(namespace: &str, local: &str) -> Option<Self> {
        let category = match (namespace, local) {
            (XSD_NS, "complexType" | "simpleType") => Self::Type,
            (XSD_NS, "element") => Self::Element,
            (XSD_NS, "attribute") => Self::Attribute,
            (XSD_NS, "group") => Self::Group,
            (XSD_NS, "attributeGroup") => Self::AttributeGroup,
            (XSD_NS, "key" | "unique") => Self::Key,
            (WSDL11_NS | WSDL20_NS, "message") => Self::Message,
            (WSDL11_NS | WSDL20_NS, "portType" | "interface") => Self::PortType,
            (WSDL11_NS | WSDL20_NS, "binding") => Self::Binding,
            _ => return None,
        };
        Some(category)
    }
}

pub(crate) enum Target {
    None,
    /// A declaration in this file whose element has one of `tags` (local
    /// name, any case) and the reference's name.
    Named {
        tags: &'static [&'static str],
        kind: RelationshipKind,
        pending: bool,
    },
    QName {
        category: Category,
        namespace: String,
        kind: RelationshipKind,
    },
}

pub(crate) struct Reference {
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub identifier: IdentifierKind,
    pub metadata: Vec<(&'static str, String)>,
    pub target: Target,
    /// One metadata entry for the relationship or pending row.
    pub usage: Option<(&'static str, String)>,
}

impl Reference {
    fn plain(start: usize, name: &str, identifier: IdentifierKind) -> Self {
        Self {
            start,
            end: start + name.len(),
            name: name.to_string(),
            identifier,
            metadata: Vec::new(),
            target: Target::None,
            usage: None,
        }
    }

    fn with_metadata(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.metadata.push((key, value.into()));
        self
    }

    fn named(
        mut self,
        tags: &'static [&'static str],
        kind: RelationshipKind,
        pending: bool,
        usage: (&'static str, &str),
    ) -> Self {
        self.target = Target::Named {
            tags,
            kind,
            pending,
        };
        self.usage = Some((usage.0, usage.1.to_string()));
        self
    }
}

pub(crate) fn references(
    base: &BaseExtractor,
    context: &XmlContext,
    tree: &Tree,
) -> Vec<Reference> {
    let content = base.content.as_str();
    let mut references = Vec::new();
    collect(content, context, tree.root_node(), &mut references, 0);
    if let (Some(dialect @ (BuildDialect::MsBuild | BuildDialect::Ant)), Some(root)) =
        (context.build, build::root_element(tree))
    {
        for site in build::target_references(dialect, content, root) {
            references.push(
                Reference::plain(site.start, &site.name, IdentifierKind::Call).named(
                    &["target"],
                    RelationshipKind::Calls,
                    true,
                    ("target_usage", site.usage),
                ),
            );
        }
        for site in build::property_references(dialect, content, root) {
            references.push(Reference::plain(
                site.start,
                &site.name,
                IdentifierKind::VariableRef,
            ));
        }
    }
    references.sort_by_key(|reference| (reference.start, reference.end));
    references
}

fn collect(
    content: &str,
    context: &XmlContext,
    node: Node<'_>,
    out: &mut Vec<Reference>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "STag" | "EmptyElemTag" => element_references(content, context, node, out),
        "EntityRef" => entity_reference(content, node, out),
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(content, context, child, out, child_depth);
    }
}

fn entity_reference(content: &str, node: Node<'_>, out: &mut Vec<Reference>) {
    let mut cursor = node.walk();
    let Some(name) = node
        .children(&mut cursor)
        .find(|child| child.kind() == "Name")
    else {
        return;
    };
    let Some(text) = content.get(name.byte_range()) else {
        return;
    };
    if PREDEFINED_ENTITIES.contains(&text) {
        return;
    }
    out.push(
        Reference::plain(name.start_byte(), text, IdentifierKind::VariableRef).named(
            &["!ENTITY"],
            RelationshipKind::References,
            false,
            ("xmlRef", "entity"),
        ),
    );
}

struct Attribute<'a> {
    name: &'a str,
    value: &'a str,
    value_start: usize,
}

fn tag_attributes<'a>(content: &'a str, tag: Node<'_>) -> Vec<Attribute<'a>> {
    let mut cursor = tag.walk();
    let mut found = Vec::new();
    for attribute in tag.children(&mut cursor) {
        if attribute.kind() != "Attribute" {
            continue;
        }
        let mut parts = attribute.walk();
        let mut name = None;
        let mut value = None;
        for part in attribute.children(&mut parts) {
            match part.kind() {
                "Name" if name.is_none() => name = content.get(part.byte_range()),
                "AttValue" => value = Some(part),
                _ => {}
            }
        }
        let (Some(name), Some(value)) = (name, value) else {
            continue;
        };
        let start = value.start_byte() + 1;
        let Some(text) = content.get(start..value.end_byte().saturating_sub(1)) else {
            continue;
        };
        found.push(Attribute {
            name,
            value: text,
            value_start: start,
        });
    }
    found
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn element_references(
    content: &str,
    context: &XmlContext,
    tag: Node<'_>,
    out: &mut Vec<Reference>,
) {
    let Some(tag_name) = tag_name_text(content, tag) else {
        return;
    };
    let tag_local = local(tag_name);
    let element = tag.parent().filter(|parent| parent.kind() == "element");
    let scope = element.unwrap_or(tag);
    if context.framework == Framework::Resx
        && std::iter::successors(Some(scope), |node| node.parent())
            .filter(|node| node.kind() == "element")
            .any(|element| build::local_tag(content, element) == Some("schema"))
    {
        return;
    }
    let attributes = tag_attributes(content, tag);
    let namespace = element_namespace(content, tag);
    let in_component = namespace
        .as_deref()
        .is_some_and(|uri| COMPONENT_ELEMENT_NAMESPACES.contains(&uri));

    for attribute in &attributes {
        let attribute_local = local(attribute.name);
        let prefixed_component = attribute.name.split_once(':').is_some_and(|(prefix, _)| {
            resolve_prefix(content, scope, Some(prefix))
                .is_some_and(|uri| COMPONENT_ATTRIBUTE_NAMESPACES.contains(&uri.as_str()))
        });
        if QNAME_ATTRIBUTES.contains(&attribute_local) && (in_component || prefixed_component) {
            qname_references(content, scope, tag_local, attribute, out);
        }
    }

    match context.framework {
        Framework::Spring => spring_references(tag_local, &attributes, out),
        Framework::MyBatis => mybatis_references(element, tag_local, &attributes, out),
        Framework::TestNg => testng_references(content, element, tag_local, &attributes, out),
        Framework::WebApp => {
            if let Some(element) = element {
                servlet_references(content, element, tag_local, out);
            }
        }
        _ => {}
    }
    if namespace.as_deref() == Some(XSLT_NS)
        && tag_local == "call-template"
        && let Some(attribute) = attributes.iter().find(|attribute| attribute.name == "name")
    {
        out.push(
            Reference::plain(attribute.value_start, attribute.value, IdentifierKind::Call).named(
                &["template"],
                RelationshipKind::Calls,
                true,
                ("xmlRef", "call-template"),
            ),
        );
    }
    if context.android || context.framework == Framework::AndroidManifest {
        android_references(
            content,
            context,
            scope,
            tag,
            tag_name,
            tag_local,
            &attributes,
            out,
        );
    }
}

/// `tns:Address` resolved through the in-scope prefixes; `memberTypes` holds
/// a whitespace-separated list.
fn qname_references(
    content: &str,
    scope: Node<'_>,
    tag_local: &str,
    attribute: &Attribute<'_>,
    out: &mut Vec<Reference>,
) {
    let attribute_local = local(attribute.name);
    let category = match (attribute_local, tag_local) {
        ("type", "binding") => Some(Category::PortType),
        ("base" | "type" | "itemType" | "memberTypes", _) => Some(Category::Type),
        ("ref", "element") => Some(Category::Element),
        ("ref", "attribute") => Some(Category::Attribute),
        ("ref", "group") => Some(Category::Group),
        ("ref", "attributeGroup") => Some(Category::AttributeGroup),
        ("element" | "substitutionGroup", _) => Some(Category::Element),
        ("refer", _) => Some(Category::Key),
        ("message", _) => Some(Category::Message),
        ("binding", _) => Some(Category::Binding),
        ("interface", _) => Some(Category::PortType),
        _ => None,
    };
    let kind = if attribute_local == "base" {
        RelationshipKind::Extends
    } else {
        RelationshipKind::References
    };
    for (offset, qname) in tokens(attribute.value, attribute_local == "memberTypes") {
        let (prefix, local_part, local_offset) = match qname.split_once(':') {
            Some((prefix, local_part)) => (Some(prefix), local_part, prefix.len() + 1),
            None => (None, qname, 0),
        };
        if local_part.is_empty() {
            continue;
        }
        let resolved = resolve_prefix(content, scope, prefix);
        let namespace = match (&resolved, prefix) {
            (Some(uri), _) => Some(uri.clone()),
            (None, None) => Some(String::new()),
            (None, Some(_)) => None,
        };
        let mut reference = Reference::plain(
            attribute.value_start + offset + local_offset,
            local_part,
            IdentifierKind::TypeUsage,
        )
        .with_metadata("qname", qname);
        if let Some(prefix) = prefix {
            reference = reference.with_metadata("prefix", prefix);
        }
        if let Some(uri) = &resolved {
            reference = reference.with_metadata("namespace", uri.as_str());
        }
        if let (Some(category), Some(namespace)) = (category, namespace)
            && namespace != XSD_NS
        {
            reference.target = Target::QName {
                category,
                namespace,
                kind: kind.clone(),
            };
            reference.usage = Some(("xmlAttribute", attribute_local.to_string()));
        }
        out.push(reference);
    }
}

/// Non-empty tokens of a value with their byte offsets: whitespace-separated
/// when `split`, else the trimmed whole value.
fn tokens(value: &str, split: bool) -> Vec<(usize, &str)> {
    if split {
        let base = value.as_ptr() as usize;
        return value
            .split_whitespace()
            .map(|token| (token.as_ptr() as usize - base, token))
            .collect();
    }
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    vec![(value.len() - value.trim_start().len(), trimmed)]
}

/// Tokens split on any of `separators`, with byte offsets.
fn split_tokens<'a>(value: &'a str, separators: &[char]) -> Vec<(usize, &'a str)> {
    let mut found = Vec::new();
    let mut offset = 0;
    for part in value.split(|ch| separators.contains(&ch)) {
        let name = part.trim();
        if !name.is_empty() {
            found.push((offset + part.len() - part.trim_start().len(), name));
        }
        offset += part.len() + 1;
    }
    found
}

/// A class-name reference: the simple name at its own span, the qualified
/// name in metadata.
fn class_reference(
    start: usize,
    qualified: &str,
    written: &str,
    attribute: &str,
) -> Option<Reference> {
    let simple_offset = written.rfind(['.', '$']).map_or(0, |index| index + 1);
    let simple = &written[simple_offset..];
    if simple.is_empty() || !simple.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_') {
        return None;
    }
    Some(
        Reference::plain(start + simple_offset, simple, IdentifierKind::TypeUsage)
            .with_metadata("qualified_name", qualified)
            .with_metadata("attribute", attribute),
    )
}

fn spring_references(tag_local: &str, attributes: &[Attribute<'_>], out: &mut Vec<Reference>) {
    for attribute in attributes {
        let value = attribute.value.trim();
        let start =
            attribute.value_start + (attribute.value.len() - attribute.value.trim_start().len());
        if value.is_empty() || value.contains("${") || value.contains("#{") {
            continue;
        }
        match attribute.name {
            "class" => out.extend(class_reference(start, value, value, "class")),
            "init-method" | "destroy-method" | "factory-method" => out.push(
                Reference::plain(start, value, IdentifierKind::Call)
                    .with_metadata("attribute", attribute.name),
            ),
            "ref" | "value-ref" | "key-ref" | "factory-bean" | "parent" => {
                out.push(bean_reference(start, value, attribute.name))
            }
            "bean" if matches!(tag_local, "ref" | "idref" | "lookup-method") => {
                out.push(bean_reference(start, value, attribute.name))
            }
            "depends-on" => {
                for (offset, name) in split_tokens(attribute.value, &[',', ';', ' ']) {
                    out.push(bean_reference(
                        attribute.value_start + offset,
                        name,
                        "depends-on",
                    ));
                }
            }
            _ => {}
        }
    }
}

fn bean_reference(start: usize, name: &str, attribute: &str) -> Reference {
    Reference::plain(start, name, IdentifierKind::VariableRef).named(
        &["bean"],
        RelationshipKind::References,
        true,
        ("springRef", attribute),
    )
}

fn mybatis_references(
    element: Option<Node<'_>>,
    tag_local: &str,
    attributes: &[Attribute<'_>],
    out: &mut Vec<Reference>,
) {
    let is_root = element.is_some_and(super::elements::is_document_root);
    for attribute in attributes {
        let value = attribute.value.trim();
        let start =
            attribute.value_start + (attribute.value.len() - attribute.value.trim_start().len());
        if value.is_empty() {
            continue;
        }
        match attribute.name {
            "namespace" if is_root && tag_local == "mapper" => {
                out.extend(class_reference(start, value, value, "namespace"))
            }
            "type" | "resultType" | "parameterType" | "ofType" | "javaType"
                if value.contains('.') =>
            {
                out.extend(class_reference(start, value, value, attribute.name))
            }
            "resultMap" | "extends" if tag_local != "mapper" => {
                for (offset, name) in split_tokens(attribute.value, &[',']) {
                    out.push(
                        Reference::plain(
                            attribute.value_start + offset,
                            name,
                            IdentifierKind::VariableRef,
                        )
                        .named(
                            &["resultMap"],
                            RelationshipKind::References,
                            true,
                            ("mybatisRef", attribute.name),
                        ),
                    );
                }
            }
            "refid" if tag_local == "include" => out.push(
                Reference::plain(start, value, IdentifierKind::VariableRef).named(
                    &["sql"],
                    RelationshipKind::References,
                    true,
                    ("mybatisRef", "include"),
                ),
            ),
            _ => {}
        }
    }
}

fn testng_references(
    content: &str,
    element: Option<Node<'_>>,
    tag_local: &str,
    attributes: &[Attribute<'_>],
    out: &mut Vec<Reference>,
) {
    let Some(name) = attributes.iter().find(|attribute| attribute.name == "name") else {
        return;
    };
    let value = name.value.trim();
    let start = name.value_start + (name.value.len() - name.value.trim_start().len());
    match tag_local {
        "class" => out.extend(class_reference(start, value, value, "name")),
        "include" | "exclude"
            if !value.is_empty()
                && element
                    .and_then(build::parent_element)
                    .is_some_and(|parent| build::local_tag(content, parent) == Some("methods")) =>
        {
            out.push(
                Reference::plain(start, value, IdentifierKind::Call)
                    .with_metadata("selection", tag_local),
            );
        }
        _ => {}
    }
}

fn servlet_references(content: &str, element: Node<'_>, tag_local: &str, out: &mut Vec<Reference>) {
    let parent =
        build::parent_element(element).and_then(|parent| build::local_tag(content, parent));
    let Some((start, text)) = text_site(content, element) else {
        return;
    };
    match (tag_local, parent) {
        ("servlet-class" | "filter-class" | "listener-class", _) => {
            out.extend(class_reference(start, text, text, tag_local))
        }
        ("servlet-name", Some("servlet-mapping" | "filter-mapping")) => out.push(
            Reference::plain(start, text, IdentifierKind::VariableRef).named(
                &["servlet"],
                RelationshipKind::References,
                false,
                ("servletRef", "servlet-name"),
            ),
        ),
        ("filter-name", Some("filter-mapping")) => out.push(
            Reference::plain(start, text, IdentifierKind::VariableRef).named(
                &["filter"],
                RelationshipKind::References,
                false,
                ("servletRef", "filter-name"),
            ),
        ),
        _ => {}
    }
}

/// The trimmed character data directly inside an element, with its start byte.
fn text_site<'a>(content: &'a str, element: Node<'_>) -> Option<(usize, &'a str)> {
    let mut cursor = element.walk();
    let body = element
        .children(&mut cursor)
        .find(|child| child.kind() == "content")?;
    let mut body_cursor = body.walk();
    let data = body
        .children(&mut body_cursor)
        .find(|node| node.kind() == "CharData" && !content[node.byte_range()].trim().is_empty())?;
    let raw = content.get(data.byte_range())?;
    let trimmed = raw.trim();
    Some((
        data.start_byte() + raw.len() - raw.trim_start().len(),
        trimmed,
    ))
}

const ANDROID_COMPONENTS: &[&str] = &[
    "application",
    "activity",
    "activity-alias",
    "service",
    "receiver",
    "provider",
];

#[allow(clippy::too_many_arguments)]
fn android_references(
    content: &str,
    context: &XmlContext,
    scope: Node<'_>,
    tag: Node<'_>,
    tag_name: &str,
    tag_local: &str,
    attributes: &[Attribute<'_>],
    out: &mut Vec<Reference>,
) {
    if tag_name.contains('.') && !tag_name.contains(':') {
        let mut cursor = tag.walk();
        if let Some(name) = tag
            .children(&mut cursor)
            .find(|child| child.kind() == "Name")
        {
            out.extend(class_reference(
                name.start_byte(),
                tag_name,
                tag_name,
                "tag",
            ));
        }
    }
    for attribute in attributes {
        let namespace = attribute
            .name
            .split_once(':')
            .and_then(|(prefix, _)| resolve_prefix(content, scope, Some(prefix)));
        let attribute_local = local(attribute.name);
        let value = attribute.value.trim();
        let start =
            attribute.value_start + (attribute.value.len() - attribute.value.trim_start().len());
        if value.is_empty() {
            continue;
        }
        let is_android = namespace.as_deref() == Some(ANDROID_NS);
        let class_attribute = match (namespace.as_deref(), attribute_local) {
            (Some(ANDROID_NS), "name")
                if context.framework == Framework::AndroidManifest
                    && ANDROID_COMPONENTS.contains(&tag_local)
                    && tag_local != "activity-alias" =>
            {
                true
            }
            (Some(ANDROID_NS), "targetActivity") => true,
            (Some(ANDROID_NS), "name")
                if matches!(
                    tag_local,
                    "fragment" | "androidx.fragment.app.FragmentContainerView"
                ) =>
            {
                true
            }
            (None, "class") if matches!(tag_local, "view" | "fragment") => true,
            (Some(ANDROID_TOOLS_NS), "context") => true,
            _ => false,
        };
        if class_attribute {
            let qualified = qualify_android_class(context.android_package.as_deref(), value);
            out.extend(class_reference(start, &qualified, value, attribute.name));
            continue;
        }
        if is_android && attribute_local == "onClick" {
            out.push(
                Reference::plain(start, value, IdentifierKind::Call)
                    .with_metadata("attribute", attribute.name),
            );
            continue;
        }
        if let Some(reference) = resource_reference(start, value) {
            out.push(reference);
        }
    }
}

fn qualify_android_class(package: Option<&str>, value: &str) -> String {
    match package {
        Some(package) if value.starts_with('.') => format!("{package}{value}"),
        Some(package) if !value.contains('.') => format!("{package}.{value}"),
        _ => value.to_string(),
    }
}

/// `@string/title`, `@android:color/white`, `@id/save`: the resource name,
/// with its type (and package) in metadata. `@+id/` declares, not references.
fn resource_reference(start: usize, value: &str) -> Option<Reference> {
    let rest = value.strip_prefix('@')?;
    if rest.starts_with('+') {
        return None;
    }
    let (qualifier, name) = rest.split_once('/')?;
    let (package, resource_type) = match qualifier.split_once(':') {
        Some((package, resource_type)) => (Some(package), resource_type),
        None => (None, qualifier),
    };
    let valid = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.'))
    };
    if !valid(resource_type) || !valid(name) {
        return None;
    }
    let mut reference = Reference::plain(
        start + value.len() - name.len(),
        name,
        IdentifierKind::VariableRef,
    )
    .with_metadata("resource_type", resource_type);
    if let Some(package) = package {
        reference = reference.with_metadata("resource_package", package);
    }
    Some(reference)
}

/// The narrowest symbol whose span holds the byte range.
pub(crate) fn containing_symbol(symbols: &[Symbol], start: usize, end: usize) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| symbol.start_byte as usize <= start && end <= symbol.end_byte as usize)
        .min_by_key(|symbol| {
            (
                symbol.end_byte - symbol.start_byte,
                std::cmp::Reverse(symbol.start_byte),
            )
        })
}

/// Resolves reference targets against the file's declarations.
pub(crate) struct Resolver<'a> {
    symbols: &'a [Symbol],
    components: HashMap<(Category, String, String), &'a Symbol>,
    import_locations: HashMap<String, String>,
}

impl<'a> Resolver<'a> {
    pub(crate) fn new(content: &str, tree: &Tree, symbols: &'a [Symbol]) -> Self {
        let mut components = HashMap::new();
        let mut import_locations = HashMap::new();
        if let Some(root) = build::root_element(tree) {
            for element in build::all_elements(root) {
                let Some(namespace) = element_namespace(content, element) else {
                    continue;
                };
                let Some(tag_local) = build::local_tag(content, element) else {
                    continue;
                };
                if tag_local == "import"
                    && let Some((_, imported)) = build::attribute(content, element, "namespace")
                    && let Some((_, location)) =
                        build::attribute(content, element, "schemaLocation")
                            .or_else(|| build::attribute(content, element, "location"))
                {
                    import_locations
                        .entry(normalize_uri(&imported))
                        .or_insert(location);
                }
                let Some(category) = Category::declared_by(&namespace, tag_local) else {
                    continue;
                };
                let parent = build::parent_element(element)
                    .and_then(|parent| build::local_tag(content, parent));
                let global = category == Category::Key
                    || matches!(
                        parent,
                        Some("schema" | "redefine" | "override" | "definitions" | "description")
                    );
                let Some((_, name)) = build::attribute(content, element, "name") else {
                    continue;
                };
                let target_namespace =
                    std::iter::successors(Some(element), |node| build::parent_element(*node))
                        .find(|node| {
                            matches!(
                                build::local_tag(content, *node),
                                Some("schema" | "definitions" | "description")
                            )
                        })
                        .and_then(|owner| build::attribute(content, owner, "targetNamespace"))
                        .map(|(_, uri)| normalize_uri(&uri))
                        .unwrap_or_default();
                let symbol = symbols.iter().find(|symbol| {
                    symbol.start_byte as usize == element.start_byte()
                        && symbol.end_byte as usize == element.end_byte()
                });
                if let (true, Some(symbol)) = (global, symbol) {
                    components
                        .entry((category, target_namespace, name))
                        .or_insert(symbol);
                }
            }
        }
        Self {
            symbols,
            components,
            import_locations,
        }
    }

    pub(crate) fn resolve(&self, reference: &Reference) -> Option<&'a Symbol> {
        match &reference.target {
            Target::None => None,
            Target::Named { tags, .. } => self.symbols.iter().find(|symbol| {
                symbol.name == reference.name
                    && symbol
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("tag"))
                        .and_then(Value::as_str)
                        .is_some_and(|tag| {
                            tags.iter()
                                .any(|wanted| local(tag).eq_ignore_ascii_case(wanted))
                        })
            }),
            Target::QName {
                category,
                namespace,
                ..
            } => self
                .components
                .get(&(*category, normalize_uri(namespace), reference.name.clone()))
                .copied(),
        }
    }

    fn import_location(&self, namespace: &str) -> Option<String> {
        self.import_locations
            .get(&normalize_uri(namespace))
            .cloned()
    }
}

/// Same-file edges for resolved references and structured pending rows for
/// references to declarations in other files.
pub(crate) fn emit_relationships(
    base: &mut BaseExtractor,
    references: &[Reference],
    resolver: &Resolver<'_>,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    for reference in references {
        let (kind, pending) = match &reference.target {
            Target::None => continue,
            Target::Named { kind, pending, .. } => (kind.clone(), *pending),
            Target::QName { kind, .. } => (kind.clone(), true),
        };
        let Some(from) = containing_symbol(symbols, reference.start, reference.end) else {
            continue;
        };
        let Some(span) = base.span_for_byte_range(reference.start, reference.end) else {
            continue;
        };
        let metadata = reference
            .usage
            .as_ref()
            .map(|(key, value)| HashMap::from([(key.to_string(), Value::String(value.clone()))]));
        if let Some(to) = resolver.resolve(reference) {
            if to.id == from.id {
                continue;
            }
            relationships.push(Relationship {
                id: format!("{}_{}_{:?}_{}", from.id, to.id, kind, reference.start),
                from_symbol_id: from.id.clone(),
                to_symbol_id: to.id.clone(),
                kind,
                file_path: base.file_path.clone(),
                line_number: span.start_line,
                span: Some(span),
                reference_site_is_exact: true,
                confidence: 1.0,
                metadata,
            });
            continue;
        }
        if !pending {
            continue;
        }
        let (namespace_path, import_context, display_name) = match &reference.target {
            Target::QName { namespace, .. } => (
                if namespace.is_empty() {
                    Vec::new()
                } else {
                    vec![namespace.clone()]
                },
                resolver.import_location(namespace),
                reference
                    .metadata
                    .iter()
                    .find(|(key, _)| *key == "qname")
                    .map_or_else(|| reference.name.clone(), |(_, qname)| qname.clone()),
            ),
            _ => (Vec::new(), None, reference.name.clone()),
        };
        let pending = StructuredPendingRelationship::new(
            from.id.clone(),
            UnresolvedTarget {
                display_name,
                terminal_name: reference.name.clone(),
                receiver: None,
                namespace_path,
                import_context,
            },
            Some(from.id.clone()),
            kind,
            base.file_path.clone(),
            span.start_line,
            1.0,
        )
        .with_target_span(span);
        base.add_structured_pending_relationship(pending);
    }
}

/// One identifier per reference site, owned by the narrowest enclosing symbol
/// and pointing at the resolved declaration when there is one.
pub(crate) fn emit_identifiers(
    base: &mut BaseExtractor,
    references: &[Reference],
    resolver: &Resolver<'_>,
    symbols: &[Symbol],
) {
    for reference in references {
        let Some(span) = base.span_for_byte_range(reference.start, reference.end) else {
            continue;
        };
        let containing = containing_symbol(symbols, reference.start, reference.end)
            .map(|symbol| symbol.id.clone());
        let metadata = (!reference.metadata.is_empty()).then(|| {
            reference
                .metadata
                .iter()
                .map(|(key, value)| (key.to_string(), Value::String(value.clone())))
                .collect()
        });
        let target = resolver.resolve(reference).map(|symbol| symbol.id.clone());
        base.create_identifier_at_span(
            span,
            reference.name.clone(),
            reference.identifier.clone(),
            containing,
            metadata,
        );
        if let Some(last) = base.identifiers.last_mut() {
            last.target_symbol_id = target;
        }
    }
}

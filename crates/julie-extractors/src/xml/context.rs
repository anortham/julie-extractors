//! Document-level XML context: the framework vocabulary a document uses and
//! in-scope namespace resolution.
//!
//! - The framework comes from the file name and the root element: a Spring
//!   `<beans>` file, a MyBatis `<mapper namespace>`, an Android manifest, a
//!   servlet `<web-app>`, a TestNG `<suite>`, an Ant `<project>` with targets,
//!   or a `.resx` resource file.
//! - Vocabularies that mix into any document (XML Schema, WSDL, XSLT, XAML,
//!   XInclude, Android resource attributes) are recognized per element by the
//!   namespace URI its prefix resolves to through the in-scope `xmlns`
//!   declarations.

use tree_sitter::{Node, Tree};

use super::build::{self, BuildDialect};

pub(crate) const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema";
pub(crate) const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";
pub(crate) const WSDL11_NS: &str = "http://schemas.xmlsoap.org/wsdl";
pub(crate) const WSDL20_NS: &str = "http://www.w3.org/ns/wsdl";
pub(crate) const XSLT_NS: &str = "http://www.w3.org/1999/XSL/Transform";
pub(crate) const XAML_NS: &str = "http://schemas.microsoft.com/winfx/2006/xaml";
pub(crate) const XINCLUDE_NS: &str = "http://www.w3.org/2001/XInclude";
pub(crate) const ANDROID_NS: &str = "http://schemas.android.com/apk/res/android";
pub(crate) const ANDROID_TOOLS_NS: &str = "http://schemas.android.com/tools";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Framework {
    Generic,
    Ant,
    Spring,
    MyBatis,
    AndroidManifest,
    WebApp,
    TestNg,
    Resx,
}

pub(crate) struct XmlContext {
    pub framework: Framework,
    pub build: Option<BuildDialect>,
    /// The `package` attribute of an Android manifest, for `.Relative` class names.
    pub android_package: Option<String>,
    /// Whether the root element binds the Android resource namespace.
    pub android: bool,
}

impl XmlContext {
    pub(crate) fn detect(tree: &Tree, file_path: &str, content: &str) -> Self {
        let root = build::root_element(tree);
        let build = BuildDialect::detect(file_path, content, root);
        let file_name = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
        let root_local = root.and_then(|root| build::local_tag(content, root));
        let has = |name: &str| root.and_then(|root| build::attribute(content, root, name));
        let framework = if file_name.to_ascii_lowercase().ends_with(".resx") {
            Framework::Resx
        } else if build == Some(BuildDialect::Ant) {
            Framework::Ant
        } else {
            match root_local {
                Some("beans") => Framework::Spring,
                Some("mapper") if has("namespace").is_some() => Framework::MyBatis,
                Some("manifest") if file_name == "AndroidManifest.xml" => {
                    Framework::AndroidManifest
                }
                Some("web-app" | "web-fragment") => Framework::WebApp,
                Some("suite") => Framework::TestNg,
                _ => Framework::Generic,
            }
        };
        let android = root.is_some_and(|root| {
            declared_namespaces(content, root)
                .iter()
                .any(|(_, uri)| uri == ANDROID_NS)
        });
        Self {
            framework,
            build,
            android_package: (framework == Framework::AndroidManifest)
                .then(|| has("package").map(|(_, package)| package))
                .flatten(),
            android,
        }
    }
}

/// The `xmlns` declarations on one element's start tag: `(prefix, uri)`, the
/// default namespace with an empty prefix.
pub(crate) fn declared_namespaces(
    content: &str,
    element_or_tag: Node<'_>,
) -> Vec<(String, String)> {
    let Some(tag) = tag_of(element_or_tag) else {
        return Vec::new();
    };
    let mut declared = Vec::new();
    let mut cursor = tag.walk();
    for attribute in tag.children(&mut cursor) {
        if attribute.kind() != "Attribute" {
            continue;
        }
        let Some((name, value)) = attribute_parts(content, attribute) else {
            continue;
        };
        let prefix = if name == "xmlns" {
            ""
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            prefix
        } else {
            continue;
        };
        declared.push((prefix.to_string(), normalize_uri(&value)));
    }
    declared
}

/// The namespace URI a prefix (or the default namespace, for `None`) resolves
/// to at `node`, by the nearest enclosing `xmlns` declaration.
pub(crate) fn resolve_prefix(
    content: &str,
    node: Node<'_>,
    prefix: Option<&str>,
) -> Option<String> {
    let wanted = prefix.unwrap_or("");
    if wanted == "xml" {
        return Some("http://www.w3.org/XML/1998/namespace".to_string());
    }
    std::iter::successors(Some(node), |node| node.parent())
        .filter(|node| matches!(node.kind(), "element" | "STag" | "EmptyElemTag"))
        .find_map(|scope| {
            declared_namespaces(content, scope)
                .into_iter()
                .find(|(declared, _)| declared == wanted)
                .map(|(_, uri)| uri)
        })
        .filter(|uri| !uri.is_empty())
}

/// The namespace URI of an element's (or orphan tag's) qualified name.
pub(crate) fn element_namespace(content: &str, element_or_tag: Node<'_>) -> Option<String> {
    let tag = tag_of(element_or_tag)?;
    let name = tag_name_text(content, tag)?;
    let prefix = name.split_once(':').map(|(prefix, _)| prefix);
    resolve_prefix(content, element_or_tag, prefix)
}

pub(crate) fn tag_of(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "element" => build::tag(node),
        "STag" | "EmptyElemTag" => Some(node),
        _ => None,
    }
}

pub(crate) fn tag_name_text<'a>(content: &'a str, tag: Node<'_>) -> Option<&'a str> {
    let mut cursor = tag.walk();
    let name = tag
        .children(&mut cursor)
        .find(|child| child.kind() == "Name")?;
    content.get(name.byte_range())
}

fn attribute_parts(content: &str, attribute: Node<'_>) -> Option<(String, String)> {
    let mut cursor = attribute.walk();
    let mut name = None;
    let mut value = None;
    for part in attribute.children(&mut cursor) {
        match part.kind() {
            "Name" if name.is_none() => name = content.get(part.byte_range()),
            "AttValue" => value = content.get(part.byte_range()),
            _ => {}
        }
    }
    let value = value?;
    let value = value.get(1..value.len().saturating_sub(1)).unwrap_or("");
    Some((name?.to_string(), value.to_string()))
}

/// WSDL 1.1 is quoted both with and without its trailing slash.
pub(crate) fn normalize_uri(uri: &str) -> String {
    uri.trim().trim_end_matches('/').to_string()
}

/// Import and package declaration extraction
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::java::JavaExtractor;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract package declaration from a node
pub(super) fn extract_package(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let package_id = node
        .children(&mut node.walk())
        .find(|child| matches!(child.kind(), "identifier" | "scoped_identifier"))?;

    let package_name = extractor.base().get_node_text(&package_id);
    let signature = format!("package {}", package_name);

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(Visibility::Public),
        parent_id: parent_id.map(|s| s.to_string()),
        ..Default::default()
    };

    Some(
        extractor
            .base_mut()
            .create_symbol(&node, package_name, SymbolKind::Namespace, options),
    )
}

/// Extract an import declaration, named by the imported type or member, or
/// by the last package segment of a wildcard import. Metadata `source` holds
/// the package or type the name comes from, `form` holds `import` or
/// `import static`, and a wildcard import has `wildcard: true`.
pub(super) fn extract_import(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let path_node = node
        .children(&mut node.walk())
        .find(|c| matches!(c.kind(), "scoped_identifier" | "identifier"))?;
    let path = extractor.base().get_node_text(&path_node);

    let is_static = node
        .children(&mut node.walk())
        .any(|c| c.kind() == "static");
    let is_wildcard = node
        .children(&mut node.walk())
        .any(|c| c.kind() == "asterisk");

    let form = if is_static { "import static" } else { "import" };
    let (symbol_name, source, signature) = if is_wildcard {
        let package_name = path.rsplit('.').next()?;
        (
            package_name.to_string(),
            Some(path.clone()),
            format!("{form} {path}.*"),
        )
    } else {
        let (container, name) = match path.rsplit_once('.') {
            Some((container, name)) => (Some(container.to_string()), name),
            None => (None, path.as_str()),
        };
        (name.to_string(), container, format!("{form} {path}"))
    };
    if symbol_name.is_empty() {
        return None;
    }

    let mut metadata = HashMap::from([(
        "form".to_string(),
        serde_json::Value::String(form.to_string()),
    )]);
    if let Some(source) = source {
        metadata.insert("source".to_string(), serde_json::Value::String(source));
    }
    if is_wildcard {
        metadata.insert("wildcard".to_string(), serde_json::Value::Bool(true));
    }

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(Visibility::Public),
        parent_id: parent_id.map(|s| s.to_string()),
        metadata: Some(metadata),
        ..Default::default()
    };

    Some(
        extractor
            .base_mut()
            .create_symbol(&node, symbol_name, SymbolKind::Import, options),
    )
}

/// Extract a `module-info.java` module declaration as a namespace symbol.
/// Its directives are published as `java.module_directive.v1` facts.
pub(super) fn extract_module(
    extractor: &mut JavaExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = extractor
        .base()
        .get_node_text(&node.child_by_field_name("name")?);
    let is_open = node
        .children(&mut node.walk())
        .any(|child| child.kind() == "open");
    let signature = if is_open {
        format!("open module {name}")
    } else {
        format!("module {name}")
    };
    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(Visibility::Public),
        parent_id: parent_id.map(|s| s.to_string()),
        doc_comment: extractor.base().find_doc_comment(&node),
        annotations: super::helpers::extract_annotations(extractor.base(), node),
        ..Default::default()
    };
    Some(
        extractor
            .base_mut()
            .create_symbol(&node, name, SymbolKind::Namespace, options),
    )
}

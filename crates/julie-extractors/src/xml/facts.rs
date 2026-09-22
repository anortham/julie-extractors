//! Framework and link facts for XML documents: document links, `<add key>`
//! configuration entries, Spring beans and component scans, servlet URL
//! mappings, Android components and permissions, MyBatis statements, and
//! TestNG test selections.

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::build::{self, all_elements, attribute, child_elements, child_text, local_tag};
use super::context::{Framework, XmlContext};
use super::links::document_links;
use crate::base::StructuralFact;
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(crate) const DOCUMENT_LINK_PATTERN_ID: &str = "xml.document_link.v1";
pub(crate) const CONFIG_ENTRY_PATTERN_ID: &str = "xml.config_entry.v1";
pub(crate) const SPRING_BEAN_PATTERN_ID: &str = "xml.spring_bean.v1";
pub(crate) const SPRING_COMPONENT_SCAN_PATTERN_ID: &str = "xml.spring_component_scan.v1";
pub(crate) const SERVLET_ROUTE_PATTERN_ID: &str = "xml.servlet_route.v1";
pub(crate) const ANDROID_COMPONENT_PATTERN_ID: &str = "xml.android_component.v1";
pub(crate) const ANDROID_PERMISSION_PATTERN_ID: &str = "xml.android_permission.v1";
pub(crate) const MYBATIS_STATEMENT_PATTERN_ID: &str = "xml.mybatis_statement.v1";
pub(crate) const TEST_SELECTION_PATTERN_ID: &str = "xml.test_selection.v1";

const MAX_SQL_CHARS: usize = 4000;

pub(crate) fn xml_facts(tree: &Tree, file_path: &str, content: &str) -> Vec<StructuralFact> {
    let context = XmlContext::detect(tree, file_path, content);
    let mut facts = Vec::new();
    for link in document_links(content, tree) {
        let mut metadata = base_metadata("document_links");
        insert_string(&mut metadata, "href", &link.href);
        insert_string(&mut metadata, "link_kind", link.kind);
        if let Some(namespace) = &link.namespace {
            insert_string(&mut metadata, "namespace", namespace);
        }
        facts.push(fact_for_node(
            file_path,
            "xml",
            DOCUMENT_LINK_PATTERN_ID,
            "document_link",
            link.node,
            metadata,
        ));
    }
    let Some(root) = build::root_element(tree) else {
        return facts;
    };
    let fact = |pattern_id, kind, node, metadata| {
        fact_for_node(file_path, "xml", pattern_id, kind, node, metadata)
    };
    for element in all_elements(root) {
        let Some(tag) = local_tag(content, element) else {
            continue;
        };
        if tag == "add"
            && attribute(content, element, "name").is_none()
            && let Some((_, key)) = attribute(content, element, "key")
        {
            let mut metadata = base_metadata("config_structure");
            insert_string(&mut metadata, "key", &key);
            if let Some((_, value)) = attribute(content, element, "value") {
                insert_string(&mut metadata, "value", &value);
            }
            if let Some(section) =
                build::parent_element(element).and_then(|p| local_tag(content, p))
            {
                insert_string(&mut metadata, "section", section);
            }
            facts.push(fact(
                CONFIG_ENTRY_PATTERN_ID,
                "config_entry",
                element,
                metadata,
            ));
        }
        match (context.framework, tag) {
            (Framework::Spring, "bean") => {
                facts.push(spring_bean_fact(file_path, content, element))
            }
            (Framework::Spring, "component-scan") => {
                if let Some((_, package)) = attribute(content, element, "base-package") {
                    let mut metadata = framework_metadata("framework", "spring");
                    insert_string(&mut metadata, "base_package", &package);
                    facts.push(fact(
                        SPRING_COMPONENT_SCAN_PATTERN_ID,
                        "component_scan",
                        element,
                        metadata,
                    ));
                }
            }
            (Framework::WebApp, "servlet-mapping" | "filter-mapping") => {
                facts.extend(servlet_route_facts(file_path, content, root, element, tag));
            }
            (
                Framework::AndroidManifest,
                "application" | "activity" | "activity-alias" | "service" | "receiver" | "provider",
            ) => {
                if let Some(fact) =
                    android_component_fact(file_path, content, &context, element, tag)
                {
                    facts.push(fact);
                }
            }
            (
                Framework::AndroidManifest,
                "uses-permission" | "uses-permission-sdk-23" | "permission",
            ) => {
                if let Some((_, permission)) = attribute(content, element, "name") {
                    let mut metadata = framework_metadata("framework", "android");
                    insert_string(&mut metadata, "permission", &permission);
                    let usage = if tag == "permission" {
                        "declares"
                    } else {
                        "uses"
                    };
                    insert_string(&mut metadata, "usage", usage);
                    facts.push(fact(
                        ANDROID_PERMISSION_PATTERN_ID,
                        "permission",
                        element,
                        metadata,
                    ));
                }
            }
            (Framework::MyBatis, "select" | "insert" | "update" | "delete" | "sql") => {
                if let Some(fact) = mybatis_statement_fact(file_path, content, root, element, tag) {
                    facts.push(fact);
                }
            }
            (Framework::TestNg, "class") => {
                if let Some(fact) = test_selection_fact(file_path, content, element) {
                    facts.push(fact);
                }
            }
            _ => {}
        }
    }
    facts
}

fn framework_metadata(
    query_family: &str,
    framework: &str,
) -> std::collections::HashMap<String, Value> {
    let mut metadata = base_metadata(query_family);
    insert_string(&mut metadata, "framework", framework);
    metadata
}

#[inline(never)]
fn spring_bean_fact(file_path: &str, content: &str, bean: Node<'_>) -> StructuralFact {
    let mut metadata = framework_metadata("framework", "spring");
    for (key, attribute_name) in [
        ("bean_id", "id"),
        ("class", "class"),
        ("scope", "scope"),
        ("init_method", "init-method"),
        ("destroy_method", "destroy-method"),
        ("factory_method", "factory-method"),
        ("factory_bean", "factory-bean"),
        ("parent", "parent"),
    ] {
        if let Some((_, value)) = attribute(content, bean, attribute_name) {
            insert_string(&mut metadata, key, &value);
        }
    }
    fact_for_node(
        file_path,
        "xml",
        SPRING_BEAN_PATTERN_ID,
        "bean",
        bean,
        metadata,
    )
}

fn servlet_route_facts(
    file_path: &str,
    content: &str,
    root: Node<'_>,
    mapping: Node<'_>,
    tag: &str,
) -> Vec<StructuralFact> {
    let (kind, name_field, class_field, declaration) = if tag == "servlet-mapping" {
        ("servlet", "servlet-name", "servlet-class", "servlet")
    } else {
        ("filter", "filter-name", "filter-class", "filter")
    };
    let Some(name) = child_text(content, mapping, name_field) else {
        return Vec::new();
    };
    let class = child_elements(root)
        .into_iter()
        .filter(|element| local_tag(content, *element) == Some(declaration))
        .find(|element| child_text(content, *element, name_field).as_deref() == Some(&name))
        .and_then(|element| child_text(content, element, class_field));
    child_elements(mapping)
        .into_iter()
        .filter(|child| local_tag(content, *child) == Some("url-pattern"))
        .filter_map(|pattern| {
            let template = build::text(content, pattern)?;
            let mut metadata = framework_metadata("framework", "java-servlet");
            insert_string(&mut metadata, "mapping_kind", kind);
            insert_string(&mut metadata, "route_template", &template);
            insert_string(
                &mut metadata,
                "normalized_route_template",
                &normalize_route_template(&template, ParamFlavor::Braces).template,
            );
            insert_string(&mut metadata, "target_name", &name);
            if let Some(class) = &class {
                insert_string(&mut metadata, "target_class", class);
            }
            Some(fact_for_node(
                file_path,
                "xml",
                SERVLET_ROUTE_PATTERN_ID,
                "route",
                pattern,
                metadata,
            ))
        })
        .collect()
}

fn android_component_fact(
    file_path: &str,
    content: &str,
    context: &XmlContext,
    element: Node<'_>,
    tag: &str,
) -> Option<StructuralFact> {
    let (_, name) = attribute(content, element, "name")?;
    let class = match context.android_package.as_deref() {
        Some(package) if name.starts_with('.') => format!("{package}{name}"),
        Some(package) if !name.contains('.') => format!("{package}.{name}"),
        _ => name,
    };
    let mut metadata = framework_metadata("framework", "android");
    insert_string(&mut metadata, "component", tag);
    insert_string(&mut metadata, "class", &class);
    if let Some((_, exported)) = attribute(content, element, "exported") {
        metadata.insert("exported".to_string(), Value::Bool(exported == "true"));
    }
    let filters: Vec<Node> = child_elements(element)
        .into_iter()
        .filter(|child| local_tag(content, *child) == Some("intent-filter"))
        .collect();
    for (key, wanted) in [
        ("intent_actions", "action"),
        ("intent_categories", "category"),
    ] {
        let values: Vec<Value> = filters
            .iter()
            .flat_map(|filter| child_elements(*filter))
            .filter(|child| local_tag(content, *child) == Some(wanted))
            .filter_map(|child| attribute(content, child, "name"))
            .map(|(_, value)| Value::String(value))
            .collect();
        if !values.is_empty() {
            metadata.insert(key.to_string(), Value::Array(values));
        }
    }
    Some(fact_for_node(
        file_path,
        "xml",
        ANDROID_COMPONENT_PATTERN_ID,
        "component",
        element,
        metadata,
    ))
}

fn mybatis_statement_fact(
    file_path: &str,
    content: &str,
    root: Node<'_>,
    statement: Node<'_>,
    operation: &str,
) -> Option<StructuralFact> {
    let (_, id) = attribute(content, statement, "id")?;
    let mut metadata = base_metadata("query_structure");
    insert_string(&mut metadata, "framework", "mybatis");
    if let Some((_, namespace)) = attribute(content, root, "namespace") {
        insert_string(&mut metadata, "namespace", &namespace);
    }
    insert_string(&mut metadata, "statement_id", &id);
    insert_string(&mut metadata, "operation", operation);
    let mut text = String::new();
    statement_text(content, statement, &mut text, 0);
    let sql: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    insert_string(
        &mut metadata,
        "sql",
        &sql.chars().take(MAX_SQL_CHARS).collect::<String>(),
    );
    for (key, attribute_name) in [
        ("parameter_type", "parameterType"),
        ("result_type", "resultType"),
        ("result_map", "resultMap"),
    ] {
        if let Some((_, value)) = attribute(content, statement, attribute_name) {
            insert_string(&mut metadata, key, &value);
        }
    }
    Some(fact_for_node(
        file_path,
        "xml",
        MYBATIS_STATEMENT_PATTERN_ID,
        "statement",
        statement,
        metadata,
    ))
}

/// The character data and CDATA under a statement, dynamic SQL tags included.
fn statement_text(content: &str, node: Node<'_>, out: &mut String, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(node.kind(), "CharData" | "CData") {
        out.push(' ');
        out.push_str(content.get(node.byte_range()).unwrap_or(""));
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        statement_text(content, child, out, child_depth);
    }
}

fn test_selection_fact(file_path: &str, content: &str, class: Node<'_>) -> Option<StructuralFact> {
    let (_, name) = attribute(content, class, "name")?;
    let mut metadata = framework_metadata("testing", "testng");
    insert_string(&mut metadata, "class", &name);
    let ancestors: Vec<Node> = std::iter::successors(build::parent_element(class), |node| {
        build::parent_element(*node)
    })
    .collect();
    for (key, wanted) in [("test", "test"), ("suite", "suite")] {
        if let Some((_, value)) = ancestors
            .iter()
            .find(|node| local_tag(content, **node) == Some(wanted))
            .and_then(|node| attribute(content, *node, "name"))
        {
            insert_string(&mut metadata, key, &value);
        }
    }
    let methods: Vec<Node> = child_elements(class)
        .into_iter()
        .filter(|child| local_tag(content, *child) == Some("methods"))
        .flat_map(child_elements)
        .collect();
    for (key, wanted) in [
        ("included_methods", "include"),
        ("excluded_methods", "exclude"),
    ] {
        let values: Vec<Value> = methods
            .iter()
            .filter(|method| local_tag(content, **method) == Some(wanted))
            .filter_map(|method| attribute(content, *method, "name"))
            .map(|(_, value)| Value::String(value))
            .collect();
        if !values.is_empty() {
            metadata.insert(key.to_string(), Value::Array(values));
        }
    }
    Some(fact_for_node(
        file_path,
        "xml",
        TEST_SELECTION_PATTERN_ID,
        "test_selection",
        class,
        metadata,
    ))
}

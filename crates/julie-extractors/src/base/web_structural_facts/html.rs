use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::{Node, Parser, Tree};

use super::css::collect_css_structural_facts_with_host;
use super::fact_builders::{
    base_metadata, child_by_kind, fact_for_node, fact_for_node_with_identity, insert_string,
    node_text,
};
use super::{
    HTML_AREA_LINK_PATTERN_ID, HTML_DATA_ATTRIBUTE_PATTERN_ID, HTML_FORM_CONTROL_PATTERN_ID,
    HTML_FORM_PATTERN_ID, HTML_LANDMARK_PATTERN_ID, HTML_LINK_PATTERN_ID, HTML_MEDIA_PATTERN_ID,
    HTML_SCRIPT_PATTERN_ID,
};
use crate::base::embedded_span::EmbeddedSpanOffset;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

#[derive(Clone, Debug)]
struct HtmlFormContext {
    id: Option<String>,
    name: Option<String>,
    action: Option<String>,
    method: String,
}

pub(super) fn collect_html_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut forms_by_id = std::collections::HashMap::new();
    register_html_forms(tree.root_node(), content, &mut forms_by_id, 0);

    let mut facts = Vec::new();
    let mut form_stack = Vec::new();
    collect_html_node(
        tree.root_node(),
        file_path,
        content,
        &forms_by_id,
        &mut form_stack,
        &mut facts,
        0,
    );
    facts
}

fn register_html_forms(
    node: Node<'_>,
    content: &str,
    forms_by_id: &mut std::collections::HashMap<String, HtmlFormContext>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "element" && html_tag_name(content, node).as_deref() == Some("form") {
        let attributes = html_element_attributes(content, node);
        let context = html_form_context(&attributes);
        if let Some(id) = context.id.as_ref() {
            forms_by_id.insert(id.clone(), context);
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        register_html_forms(child, content, forms_by_id, child_depth);
    }
}

fn collect_html_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    forms_by_id: &std::collections::HashMap<String, HtmlFormContext>,
    form_stack: &mut Vec<HtmlFormContext>,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "script_element" => {
            let attributes = html_element_attributes(content, node);
            if let Some(fact) = html_script_fact(file_path, content, node, "script", &attributes) {
                facts.push(fact);
            }
        }
        "style_element" => {
            facts.extend(html_style_css_facts(file_path, content, node));
        }
        "element" => {
            if let Some(tag_name) = html_tag_name(content, node) {
                let attributes = html_element_attributes(content, node);
                let mut form_context = None;
                match tag_name.as_str() {
                    "a" => {
                        if let Some(fact) =
                            html_link_fact(file_path, content, node, &tag_name, &attributes)
                        {
                            facts.push(fact);
                        }
                    }
                    "area" => {
                        if let Some(fact) =
                            html_area_link_fact(file_path, content, node, &tag_name, &attributes)
                        {
                            facts.push(fact);
                        }
                    }
                    "img" | "source" | "audio" | "video" | "track" => {
                        if let Some(fact) =
                            html_media_fact(file_path, content, node, &tag_name, &attributes)
                        {
                            facts.push(fact);
                        }
                    }
                    "script" => {
                        if let Some(fact) =
                            html_script_fact(file_path, content, node, &tag_name, &attributes)
                        {
                            facts.push(fact);
                        }
                    }
                    "form" => {
                        let control_count = count_html_form_controls(node, content);
                        if let Some(fact) = html_form_fact(
                            file_path,
                            content,
                            node,
                            &tag_name,
                            &attributes,
                            control_count,
                        ) {
                            facts.push(fact);
                        }
                        form_context = Some(html_form_context(&attributes));
                    }
                    "input" | "button" | "select" | "textarea" => {
                        let owner = html_form_control_owner(&attributes, form_stack, forms_by_id);
                        if let Some(fact) = html_form_control_fact(
                            file_path,
                            content,
                            node,
                            &tag_name,
                            &attributes,
                            owner,
                        ) {
                            facts.push(fact);
                        }
                    }
                    _ => {}
                }

                if let Some(fact) =
                    html_landmark_fact_for_element(file_path, content, node, &tag_name, &attributes)
                {
                    facts.push(fact);
                }
                facts.extend(html_data_attribute_facts(
                    file_path,
                    content,
                    node,
                    &tag_name,
                    &attributes,
                ));

                if let Some(context) = form_context {
                    form_stack.push(context);
                    if let Some(child_depth) = child_tree_depth(depth) {
                        let mut cursor = node.walk();
                        for child in node.children(&mut cursor) {
                            collect_html_node(
                                child,
                                file_path,
                                content,
                                forms_by_id,
                                form_stack,
                                facts,
                                child_depth,
                            );
                        }
                    }
                    form_stack.pop();
                    return;
                }
            }
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_html_node(
            child,
            file_path,
            content,
            forms_by_id,
            form_stack,
            facts,
            child_depth,
        );
    }
}

fn html_has_landmark_role(attributes: &std::collections::HashMap<String, String>) -> bool {
    attributes.get("role").is_some_and(|roles| {
        roles.split_ascii_whitespace().any(|role| {
            matches!(
                role.to_ascii_lowercase().as_str(),
                "banner"
                    | "complementary"
                    | "contentinfo"
                    | "form"
                    | "main"
                    | "navigation"
                    | "region"
                    | "search"
            )
        })
    })
}

fn html_link_fact(
    file_path: &str,
    _content: &str,
    node: Node<'_>,
    tag_name: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> Option<StructuralFact> {
    let href = attributes.get("href")?;
    let mut metadata = base_metadata("document_navigation");
    insert_string(&mut metadata, "tag_name", tag_name);
    insert_string(&mut metadata, "href", href);
    insert_optional_string(&mut metadata, "id", attributes.get("id"));
    insert_optional_string(&mut metadata, "class", attributes.get("class"));
    insert_optional_string(&mut metadata, "rel", attributes.get("rel"));

    Some(fact_for_node(
        file_path,
        "html",
        HTML_LINK_PATTERN_ID,
        "link",
        node,
        metadata,
    ))
}

fn html_area_link_fact(
    file_path: &str,
    _content: &str,
    node: Node<'_>,
    tag_name: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> Option<StructuralFact> {
    let href = attributes.get("href")?;
    let mut metadata = base_metadata("document_navigation");
    insert_string(&mut metadata, "tag_name", tag_name);
    insert_string(&mut metadata, "href", href);
    insert_optional_string(&mut metadata, "alt", attributes.get("alt"));
    insert_optional_string(&mut metadata, "shape", attributes.get("shape"));
    insert_optional_string(&mut metadata, "coords", attributes.get("coords"));
    insert_optional_string(&mut metadata, "id", attributes.get("id"));

    Some(fact_for_node(
        file_path,
        "html",
        HTML_AREA_LINK_PATTERN_ID,
        "area_link",
        node,
        metadata,
    ))
}

fn html_media_fact(
    file_path: &str,
    _content: &str,
    node: Node<'_>,
    tag_name: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> Option<StructuralFact> {
    let src = attributes.get("src")?;
    let mut metadata = base_metadata("document_assets");
    insert_string(&mut metadata, "tag_name", tag_name);
    insert_string(&mut metadata, "src", src);
    insert_optional_string(&mut metadata, "type", attributes.get("type"));
    insert_optional_string(&mut metadata, "alt", attributes.get("alt"));
    insert_optional_string(&mut metadata, "id", attributes.get("id"));

    Some(fact_for_node(
        file_path,
        "html",
        HTML_MEDIA_PATTERN_ID,
        "media",
        node,
        metadata,
    ))
}

fn html_landmark_fact_for_element(
    file_path: &str,
    content: &str,
    node: Node<'_>,
    tag_name: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> Option<StructuralFact> {
    let is_native_landmark = matches!(tag_name, "header" | "nav" | "main" | "aside" | "footer");
    (is_native_landmark || html_has_landmark_role(attributes))
        .then(|| html_landmark_fact(file_path, content, node, tag_name, attributes))
}

fn html_style_css_facts(file_path: &str, content: &str, node: Node<'_>) -> Vec<StructuralFact> {
    let Some(raw_text) = child_by_kind(node, "raw_text") else {
        return Vec::new();
    };
    let start = raw_text.start_byte();
    let end = raw_text.end_byte();
    if start >= end || end > content.len() {
        return Vec::new();
    }
    let style_content = &content[start..end];

    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_css::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(style_content, None) else {
        return Vec::new();
    };
    let Some(offset) = EmbeddedSpanOffset::from_host_byte(content, start) else {
        return Vec::new();
    };
    collect_css_structural_facts_with_host(&tree, file_path, style_content, "html", Some(offset))
}

fn html_landmark_fact(
    file_path: &str,
    _content: &str,
    node: Node<'_>,
    tag_name: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> StructuralFact {
    let mut metadata = base_metadata("document_landmarks");
    insert_string(&mut metadata, "tag_name", tag_name);
    insert_optional_string(&mut metadata, "role", attributes.get("role"));
    insert_optional_string(&mut metadata, "id", attributes.get("id"));
    insert_optional_string(&mut metadata, "aria_label", attributes.get("aria-label"));

    fact_for_node(
        file_path,
        "html",
        HTML_LANDMARK_PATTERN_ID,
        "landmark",
        node,
        metadata,
    )
}

fn html_data_attribute_facts(
    file_path: &str,
    _content: &str,
    node: Node<'_>,
    tag_name: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> Vec<StructuralFact> {
    let mut names = attributes
        .keys()
        .filter(|name| name.starts_with("data-"))
        // Keep htmx (`data-hx-*`) and Alpine (`data-x-*` reserved / x-* primary) out of this
        // generic surface so they do not collide with framework facts.
        .filter(|name| !name.starts_with("data-hx-") && !name.starts_with("data-x-"))
        .collect::<Vec<_>>();
    names.sort_unstable();

    names
        .into_iter()
        .map(|attribute_name| {
            let mut metadata = base_metadata("document_attributes");
            insert_string(&mut metadata, "tag_name", tag_name);
            insert_string(&mut metadata, "attribute_name", attribute_name);
            insert_string(&mut metadata, "value", &attributes[attribute_name]);
            fact_for_node_with_identity(
                file_path,
                "html",
                HTML_DATA_ATTRIBUTE_PATTERN_ID,
                "data_attribute",
                attribute_name,
                node,
                metadata,
            )
        })
        .collect()
}

fn html_script_fact(
    file_path: &str,
    _content: &str,
    node: Node<'_>,
    tag_name: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> Option<StructuralFact> {
    let mut metadata = base_metadata("document_assets");
    insert_string(&mut metadata, "tag_name", tag_name);
    metadata.insert(
        "inline".to_string(),
        Value::Bool(!attributes.contains_key("src")),
    );
    insert_optional_string(&mut metadata, "src", attributes.get("src"));
    insert_optional_string(&mut metadata, "type", attributes.get("type"));
    insert_optional_string(&mut metadata, "id", attributes.get("id"));

    Some(fact_for_node(
        file_path,
        "html",
        HTML_SCRIPT_PATTERN_ID,
        "script",
        node,
        metadata,
    ))
}

fn html_form_context(attributes: &std::collections::HashMap<String, String>) -> HtmlFormContext {
    HtmlFormContext {
        id: attributes.get("id").cloned(),
        name: attributes.get("name").cloned(),
        action: attributes.get("action").cloned(),
        method: html_normalized_form_method(attributes),
    }
}

fn html_normalized_form_method(attributes: &std::collections::HashMap<String, String>) -> String {
    attributes
        .get("method")
        .map(|method| method.trim().to_ascii_lowercase())
        .filter(|method| !method.is_empty())
        .unwrap_or_else(|| "get".to_string())
}

fn html_form_method_source(attributes: &std::collections::HashMap<String, String>) -> &'static str {
    if attributes
        .get("method")
        .is_some_and(|method| !method.trim().is_empty())
    {
        "explicit"
    } else {
        "default"
    }
}

fn html_form_action_kind(action: Option<&str>) -> &'static str {
    match action.filter(|value| !value.is_empty()) {
        Some(value) if html_is_static_path(value) => "static_path",
        Some(_) => "other",
        None => "same_document",
    }
}

fn html_is_static_path(value: &str) -> bool {
    value.starts_with('/') || value.starts_with("./") || value.starts_with("../")
}

fn count_html_form_controls(node: Node<'_>, content: &str) -> usize {
    let mut count = 0;
    count_html_form_controls_node(node, content, &mut count, 0);
    count
}

fn count_html_form_controls_node(node: Node<'_>, content: &str, count: &mut usize, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "element"
        && let Some(tag_name) = html_tag_name(content, node)
        && matches!(
            tag_name.as_str(),
            "input" | "button" | "select" | "textarea"
        )
    {
        *count += 1;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count_html_form_controls_node(child, content, count, child_depth);
    }
}

fn html_form_control_owner<'a>(
    attributes: &std::collections::HashMap<String, String>,
    form_stack: &'a [HtmlFormContext],
    forms_by_id: &'a std::collections::HashMap<String, HtmlFormContext>,
) -> Option<&'a HtmlFormContext> {
    if let Some(form_id) = attributes.get("form").filter(|value| !value.is_empty())
        && let Some(owner) = forms_by_id.get(form_id)
    {
        return Some(owner);
    }
    form_stack.last()
}

fn html_form_fact(
    file_path: &str,
    _content: &str,
    node: Node<'_>,
    tag_name: &str,
    attributes: &std::collections::HashMap<String, String>,
    control_count: usize,
) -> Option<StructuralFact> {
    let method = html_normalized_form_method(attributes);
    let action = attributes.get("action").filter(|value| !value.is_empty());

    let mut metadata = base_metadata("document_forms");
    insert_string(&mut metadata, "tag_name", tag_name);
    insert_optional_string(&mut metadata, "action", attributes.get("action"));
    insert_string(&mut metadata, "method", &method);
    insert_string(
        &mut metadata,
        "method_source",
        html_form_method_source(attributes),
    );
    insert_string(
        &mut metadata,
        "action_kind",
        html_form_action_kind(action.map(String::as_str)),
    );
    if let Some(action) = action.filter(|value| html_is_static_path(value)) {
        insert_string(&mut metadata, "target_path", action);
    }
    insert_optional_string(&mut metadata, "id", attributes.get("id"));
    insert_optional_string(&mut metadata, "name", attributes.get("name"));
    insert_optional_string(&mut metadata, "enctype", attributes.get("enctype"));
    insert_optional_string(&mut metadata, "target", attributes.get("target"));
    insert_optional_string(
        &mut metadata,
        "autocomplete",
        attributes.get("autocomplete"),
    );
    metadata.insert(
        "novalidate".to_string(),
        Value::Bool(attributes.contains_key("novalidate")),
    );
    metadata.insert(
        "control_count".to_string(),
        Value::Number(Number::from(control_count)),
    );

    Some(fact_for_node(
        file_path,
        "html",
        HTML_FORM_PATTERN_ID,
        "form",
        node,
        metadata,
    ))
}

fn html_form_control_fact(
    file_path: &str,
    _content: &str,
    node: Node<'_>,
    tag_name: &str,
    attributes: &std::collections::HashMap<String, String>,
    owner: Option<&HtmlFormContext>,
) -> Option<StructuralFact> {
    let mut metadata = base_metadata("document_forms");
    insert_string(&mut metadata, "tag_name", tag_name);
    insert_optional_string(&mut metadata, "type", attributes.get("type"));
    insert_optional_string(&mut metadata, "name", attributes.get("name"));
    insert_optional_string(&mut metadata, "id", attributes.get("id"));
    insert_optional_string(&mut metadata, "value", attributes.get("value"));
    metadata.insert(
        "required".to_string(),
        Value::Bool(attributes.contains_key("required")),
    );
    insert_present_bool_attribute(&mut metadata, "disabled", attributes);
    insert_present_bool_attribute(&mut metadata, "readonly", attributes);
    insert_present_bool_attribute(&mut metadata, "checked", attributes);
    insert_present_bool_attribute(&mut metadata, "multiple", attributes);

    if let Some(owner) = owner {
        insert_optional_string(&mut metadata, "form_id", owner.id.as_ref());
        insert_optional_string(&mut metadata, "form_name", owner.name.as_ref());
        insert_optional_string(&mut metadata, "form_action", owner.action.as_ref());
        insert_string(&mut metadata, "form_method", &owner.method);
    }

    Some(fact_for_node(
        file_path,
        "html",
        HTML_FORM_CONTROL_PATTERN_ID,
        "form_control",
        node,
        metadata,
    ))
}

fn insert_present_bool_attribute(
    metadata: &mut HashMap<String, Value>,
    key: &str,
    attributes: &std::collections::HashMap<String, String>,
) {
    if attributes.contains_key(key) {
        metadata.insert(key.to_string(), Value::Bool(true));
    }
}

fn html_tag_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !matches!(child.kind(), "start_tag" | "self_closing_tag") {
            continue;
        }
        let mut tag_cursor = child.walk();
        for tag_child in child.children(&mut tag_cursor) {
            if tag_child.kind() == "tag_name" {
                return node_text(content, tag_child).map(str::to_ascii_lowercase);
            }
        }
    }
    None
}

fn html_element_attributes(
    content: &str,
    node: Node<'_>,
) -> std::collections::HashMap<String, String> {
    let mut attributes = std::collections::HashMap::new();
    let mut cursor = node.walk();
    let tag_container = node
        .children(&mut cursor)
        .find(|child| matches!(child.kind(), "start_tag" | "self_closing_tag"))
        .unwrap_or(node);

    let mut tag_cursor = tag_container.walk();
    for child in tag_container.children(&mut tag_cursor) {
        if child.kind() != "attribute" {
            continue;
        }
        if let Some((name, value)) = html_attribute_name_value(content, child) {
            attributes.insert(name, value);
        }
    }
    attributes
}

fn html_attribute_name_value(content: &str, attr_node: Node<'_>) -> Option<(String, String)> {
    let mut name = None;
    let mut value = String::new();

    let mut cursor = attr_node.walk();
    for child in attr_node.children(&mut cursor) {
        match child.kind() {
            "attribute_name" => {
                name = node_text(content, child).map(str::to_string);
            }
            "attribute_value" | "quoted_attribute_value" => {
                value = node_text(content, child)
                    .unwrap_or_default()
                    .trim_matches(|ch| ch == '"' || ch == '\'')
                    .to_string();
            }
            _ => {}
        }
    }

    name.map(|name| (name.to_ascii_lowercase(), value))
}

fn insert_optional_string(
    metadata: &mut HashMap<String, Value>,
    key: &str,
    value: Option<&String>,
) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        insert_string(metadata, key, value);
    }
}

use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::span::NormalizedSpan;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol, stable_location_id};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const CSS_SELECTOR_RULE_PATTERN_ID: &str = "css.selector_rule.v1";
const CSS_CUSTOM_PROPERTY_PATTERN_ID: &str = "css.custom_property.v1";
const CSS_MEDIA_QUERY_PATTERN_ID: &str = "css.media_query.v1";
const CSS_KEYFRAMES_PATTERN_ID: &str = "css.keyframes.v1";
const HTML_LINK_PATTERN_ID: &str = "html.link.v1";
const HTML_SCRIPT_PATTERN_ID: &str = "html.script.v1";
const HTML_FORM_PATTERN_ID: &str = "html.form.v1";
const HTML_FORM_CONTROL_PATTERN_ID: &str = "html.form_control.v1";
const VUE_SFC_SECTION_PATTERN_ID: &str = "vue.sfc_section.v1";
const VUE_TEMPLATE_DIRECTIVE_PATTERN_ID: &str = "vue.template_directive.v1";
const VUE_ROUTE_REFERENCE_PATTERN_ID: &str = "vue.route_reference.v1";
const VUE_ROUTE_DEFINITION_PATTERN_ID: &str = "vue.route_definition.v1";
const REACT_ROUTE_REFERENCE_PATTERN_ID: &str = "react.route_reference.v1";
const REACT_ROUTE_DEFINITION_PATTERN_ID: &str = "react.route_definition.v1";
const NEXTJS_ROUTE_REFERENCE_PATTERN_ID: &str = "nextjs.route_reference.v1";
const NEXTJS_FILE_ROUTE_PATTERN_ID: &str = "nextjs.file_route.v1";
const NUXT_ROUTE_REFERENCE_PATTERN_ID: &str = "nuxt.route_reference.v1";
const NUXT_FILE_ROUTE_PATTERN_ID: &str = "nuxt.file_route.v1";

#[cfg(all(test, feature = "test-capability-matrix"))]
const CSS_WEB_PATTERN_IDS: &[&str] = &[
    CSS_CUSTOM_PROPERTY_PATTERN_ID,
    CSS_KEYFRAMES_PATTERN_ID,
    CSS_MEDIA_QUERY_PATTERN_ID,
    CSS_SELECTOR_RULE_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const HTML_WEB_PATTERN_IDS: &[&str] = &[
    HTML_FORM_CONTROL_PATTERN_ID,
    HTML_FORM_PATTERN_ID,
    HTML_LINK_PATTERN_ID,
    HTML_SCRIPT_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const VUE_WEB_PATTERN_IDS: &[&str] = &[
    NUXT_FILE_ROUTE_PATTERN_ID,
    NUXT_ROUTE_REFERENCE_PATTERN_ID,
    VUE_ROUTE_DEFINITION_PATTERN_ID,
    VUE_ROUTE_REFERENCE_PATTERN_ID,
    VUE_SFC_SECTION_PATTERN_ID,
    VUE_TEMPLATE_DIRECTIVE_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const JS_FRAMEWORK_WEB_PATTERN_IDS: &[&str] = &[
    NEXTJS_FILE_ROUTE_PATTERN_ID,
    NEXTJS_ROUTE_REFERENCE_PATTERN_ID,
    NUXT_FILE_ROUTE_PATTERN_ID,
    REACT_ROUTE_DEFINITION_PATTERN_ID,
    REACT_ROUTE_REFERENCE_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const TS_FRAMEWORK_WEB_PATTERN_IDS: &[&str] = &[
    NEXTJS_FILE_ROUTE_PATTERN_ID,
    NUXT_FILE_ROUTE_PATTERN_ID,
    REACT_ROUTE_DEFINITION_PATTERN_ID,
];

pub fn collect_web_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let mut facts = match language {
        "css" => collect_css_structural_facts(tree, file_path, content),
        "html" => collect_html_structural_facts(tree, file_path, content),
        "vue" => collect_vue_structural_facts(tree, file_path, content),
        "javascript" | "jsx" | "typescript" | "tsx" => {
            collect_react_nextjs_structural_facts(language, tree, file_path, content)
        }
        _ => Vec::new(),
    };

    attach_containing_symbols(&mut facts, symbols);
    sort_structural_facts(&mut facts);
    facts
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn web_structural_fact_pattern_ids_for_language(
    language: &str,
) -> &'static [&'static str] {
    match language {
        "css" => CSS_WEB_PATTERN_IDS,
        "html" => HTML_WEB_PATTERN_IDS,
        "vue" => VUE_WEB_PATTERN_IDS,
        "javascript" | "jsx" | "tsx" => JS_FRAMEWORK_WEB_PATTERN_IDS,
        "typescript" => TS_FRAMEWORK_WEB_PATTERN_IDS,
        _ => &[],
    }
}

fn collect_css_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_css_node(tree.root_node(), file_path, content, &mut facts, 0);
    facts
}

fn collect_css_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "rule_set" => {
            if let Some(fact) = css_selector_rule_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "property_name" => {
            if let Some(fact) = css_custom_property_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "media_statement" => {
            if let Some(fact) = css_media_query_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "keyframes_statement" => {
            if let Some(fact) = css_keyframes_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_css_node(child, file_path, content, facts, child_depth);
    }
}

fn css_selector_rule_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let selectors = child_by_kind(node, "selectors")?;
    let selector_text = node_text(content, selectors)?.trim().to_string();
    if selector_text.is_empty() {
        return None;
    }

    let mut metadata = base_metadata("stylesheet_structure");
    insert_string(&mut metadata, "selector", &selector_text);
    insert_string(
        &mut metadata,
        "selector_kind",
        css_selector_kind(&selector_text),
    );
    metadata.insert(
        "declaration_count".to_string(),
        Value::Number(Number::from(count_css_declarations(node))),
    );

    Some(fact_for_node(
        file_path,
        "css",
        CSS_SELECTOR_RULE_PATTERN_ID,
        "rule_set",
        node,
        metadata,
    ))
}

fn css_custom_property_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let property_name = node_text(content, node)?.trim();
    if !property_name.starts_with("--") {
        return None;
    }

    let mut metadata = base_metadata("stylesheet_structure");
    insert_string(&mut metadata, "property_name", property_name);

    Some(fact_for_node(
        file_path,
        "css",
        CSS_CUSTOM_PROPERTY_PATTERN_ID,
        "custom_property",
        node,
        metadata,
    ))
}

fn css_media_query_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let query = css_at_rule_prelude(text, "@media");
    let mut metadata = base_metadata("responsive_design");
    if let Some(query) = query {
        insert_string(&mut metadata, "query", query);
    }

    Some(fact_for_node(
        file_path,
        "css",
        CSS_MEDIA_QUERY_PATTERN_ID,
        "media_query",
        node,
        metadata,
    ))
}

fn css_keyframes_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let animation_name = css_at_rule_prelude(text, "@keyframes");
    let mut metadata = base_metadata("animation");
    if let Some(animation_name) = animation_name {
        insert_string(&mut metadata, "animation_name", animation_name);
    }

    Some(fact_for_node(
        file_path,
        "css",
        CSS_KEYFRAMES_PATTERN_ID,
        "keyframes",
        node,
        metadata,
    ))
}

#[derive(Clone, Debug)]
struct HtmlFormContext {
    id: Option<String>,
    name: Option<String>,
    action: Option<String>,
    method: String,
}

fn collect_html_structural_facts(
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
        "element" => {
            if let Some(tag_name) = html_tag_name(content, node) {
                let attributes = html_element_attributes(content, node);
                match tag_name.as_str() {
                    "a" => {
                        if let Some(fact) =
                            html_link_fact(file_path, content, node, &tag_name, &attributes)
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
                        let context = html_form_context(&attributes);
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

fn collect_vue_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();

    if let Some(fact) = nuxt_file_route_fact("vue", file_path, content) {
        facts.push(fact);
    }

    for section in scan_vue_sections(content) {
        facts.push(vue_section_fact(file_path, &section));

        if section.section_type == "template" {
            facts.extend(collect_nuxt_route_references(file_path, content, &section));
            for attribute in
                scan_markup_attributes(content, section.content_start, section.content_end)
            {
                let directive = parse_vue_directive(&attribute.name);
                if let Some(route_fact) =
                    vue_route_reference_fact(file_path, &attribute, directive.as_ref())
                {
                    facts.push(route_fact);
                }
                if let Some(directive) = directive {
                    facts.push(vue_template_directive_fact(
                        file_path, &attribute, directive,
                    ));
                }
            }
        } else if section.section_type == "script" {
            facts.extend(collect_vue_route_definitions(
                tree, file_path, content, &section,
            ));
        }
    }

    facts
}

fn vue_section_fact(file_path: &str, section: &VueSectionSpan) -> StructuralFact {
    let mut metadata = base_metadata("component_structure");
    insert_string(&mut metadata, "section_type", section.section_type);
    if let Some(lang) = section.lang.as_deref() {
        insert_string(&mut metadata, "lang", lang);
    }
    metadata.insert("setup".to_string(), Value::Bool(section.setup));
    metadata.insert("scoped".to_string(), Value::Bool(section.scoped));

    fact_for_span(
        file_path,
        "vue",
        VUE_SFC_SECTION_PATTERN_ID,
        "section",
        "sfc_section",
        section.start_span,
        metadata,
    )
}

fn vue_template_directive_fact(
    file_path: &str,
    attribute: &MarkupAttribute,
    directive: VueDirective,
) -> StructuralFact {
    let mut metadata = base_metadata("component_template");
    insert_string(&mut metadata, "directive", directive.name);
    insert_string(&mut metadata, "attribute_name", &attribute.name);
    metadata.insert("shorthand".to_string(), Value::Bool(directive.shorthand));
    if let Some(argument) = directive.argument {
        insert_string(&mut metadata, "argument", &argument);
    }
    if !directive.modifiers.is_empty() {
        metadata.insert(
            "modifiers".to_string(),
            Value::Array(directive.modifiers.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(expression) = attribute.value.as_deref() {
        insert_string(&mut metadata, "expression", expression);
    }

    fact_for_span(
        file_path,
        "vue",
        VUE_TEMPLATE_DIRECTIVE_PATTERN_ID,
        "directive",
        "template_attribute",
        attribute.span,
        metadata,
    )
}

fn vue_route_reference_fact(
    file_path: &str,
    attribute: &MarkupAttribute,
    directive: Option<&VueDirective>,
) -> Option<StructuralFact> {
    let reference = vue_route_reference(attribute, directive)?;
    let mut metadata = base_metadata("frontend_navigation");
    insert_string(&mut metadata, "framework", "vue");
    insert_string(&mut metadata, "source_kind", reference.source_kind);
    insert_string(&mut metadata, "target_path", &reference.target_path);
    insert_string(&mut metadata, "verb", "GET");
    insert_string(&mut metadata, "attribute_name", &attribute.name);
    if let Some(expression) = reference.expression.as_deref() {
        insert_string(&mut metadata, "expression", expression);
    }

    Some(fact_for_span(
        file_path,
        "vue",
        VUE_ROUTE_REFERENCE_PATTERN_ID,
        "route_reference",
        "template_attribute",
        attribute.span,
        metadata,
    ))
}

fn collect_nuxt_route_references(
    file_path: &str,
    content: &str,
    section: &VueSectionSpan,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut cursor = section.content_start;

    while cursor < section.content_end {
        let Some((tag_start, tag_end, tag_name)) =
            next_markup_tag(content, cursor, section.content_end)
        else {
            break;
        };
        cursor = tag_end + 1;
        if !is_nuxt_link_tag(tag_name) {
            continue;
        }

        let mut attributes = Vec::new();
        scan_tag_attributes(content, tag_start, tag_end, &mut attributes);
        if attributes
            .iter()
            .any(|attribute| is_nuxt_external_attribute(&attribute.name))
        {
            continue;
        }

        let Some(attribute) = attributes.iter().find(|attribute| attribute.name == "to") else {
            continue;
        };
        let Some(target_path) = attribute
            .value
            .as_deref()
            .map(str::trim)
            .filter(|value| is_nuxt_route_path(value))
        else {
            continue;
        };

        let mut metadata = base_metadata("frontend_navigation");
        insert_string(&mut metadata, "framework", "nuxt");
        insert_string(&mut metadata, "target_path", target_path);
        insert_string(&mut metadata, "verb", "GET");
        insert_string(&mut metadata, "attribute_name", "to");
        insert_string(&mut metadata, "component_name", tag_name);
        insert_string(&mut metadata, "route_source", "string_literal");
        insert_string(&mut metadata, "source_kind", "nuxt_link");

        facts.push(fact_for_span(
            file_path,
            "vue",
            NUXT_ROUTE_REFERENCE_PATTERN_ID,
            "route_reference",
            "template_attribute",
            attribute.span,
            metadata,
        ));
    }

    facts
}

fn collect_vue_route_definitions(
    tree: &Tree,
    file_path: &str,
    content: &str,
    section: &VueSectionSpan,
) -> Vec<StructuralFact> {
    let imports = collect_vue_static_imports(content, section);
    let mut facts = Vec::new();
    let ranges = vue_route_definition_ranges(content, section);

    for (range_start, range_end) in ranges {
        let mut cursor = range_start;
        while cursor < range_end {
            let Some(relative_path_start) = content[cursor..range_end].find("path") else {
                break;
            };
            let path_start = cursor + relative_path_start;
            cursor = path_start + "path".len();

            if !is_identifier_boundary(content, path_start, "path".len()) {
                continue;
            }
            if is_ignored_syntax_range(tree, path_start, cursor) {
                continue;
            }
            let colon = skip_ascii_whitespace_until(content, cursor, range_end);
            if content.as_bytes().get(colon) != Some(&b':') {
                continue;
            }
            let value_start = skip_ascii_whitespace_until(content, colon + 1, range_end);
            let Some((target_path, path_end)) = parse_js_string_literal(content, value_start)
                .filter(|(value, end)| *end <= range_end && is_static_route_path(value))
            else {
                continue;
            };

            let (span_start, span_end) =
                find_enclosing_object_range(content, range_start, range_end, path_start)
                    .unwrap_or((path_start, path_end));
            let Some(span) = NormalizedSpan::from_content_range(content, span_start, span_end)
            else {
                continue;
            };

            let route_name = parse_object_string_property(content, span_start, span_end, "name");
            let component_name =
                parse_object_identifier_property(content, span_start, span_end, "component");
            let component_path = component_name
                .as_ref()
                .and_then(|name| imports.get(name))
                .cloned();

            let mut metadata = base_metadata("frontend_navigation");
            insert_string(&mut metadata, "framework", "vue");
            insert_string(&mut metadata, "target_path", &target_path);
            insert_string(&mut metadata, "source_kind", "vue_router_route");
            insert_string(&mut metadata, "route_source", "string_literal");
            if let Some(route_name) = route_name {
                insert_string(&mut metadata, "route_name", &route_name);
            }
            if let Some(component_name) = component_name {
                insert_string(&mut metadata, "component_name", &component_name);
            }
            if let Some(component_path) = component_path {
                insert_string(&mut metadata, "component_path", &component_path);
            }

            facts.push(fact_for_span(
                file_path,
                "vue",
                VUE_ROUTE_DEFINITION_PATTERN_ID,
                "route_definition",
                "object",
                span,
                metadata,
            ));
        }
    }

    facts
}

fn vue_route_definition_ranges(content: &str, section: &VueSectionSpan) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();

    if let Some(range) = find_js_array_initializer_range_in(
        content,
        "routes",
        section.content_start,
        section.content_end,
    ) {
        ranges.push(range);
    }

    let mut cursor = section.content_start;
    while cursor < section.content_end {
        let Some(relative_start) = content[cursor..section.content_end].find("createRouter") else {
            break;
        };
        let api_start = cursor + relative_start;
        cursor = api_start + "createRouter".len();
        if !is_identifier_boundary(content, api_start, "createRouter".len()) {
            continue;
        }
        let open_paren = skip_ascii_whitespace_until(content, cursor, section.content_end);
        if content.as_bytes().get(open_paren) != Some(&b'(') {
            continue;
        }
        let Some(close_paren) = find_matching_paren(content, open_paren, section.content_end)
        else {
            continue;
        };
        ranges.push((open_paren + 1, close_paren));
    }

    ranges.sort_unstable();
    ranges.dedup();
    ranges
}

#[derive(Debug, Default)]
struct JsImportIndex {
    react_router_links: HashMap<String, String>,
    react_router_routes: HashMap<String, String>,
    react_router_route_apis: HashMap<String, String>,
    next_links: HashMap<String, String>,
}

#[derive(Debug)]
struct JsxAttributeSpan {
    value_start: Option<usize>,
    value_end: usize,
    span: NormalizedSpan,
}

#[derive(Debug)]
struct NextFileRoute {
    router: &'static str,
    route_path: String,
    normalized_route_template: Option<String>,
    dynamic_segments: Vec<String>,
    route_group_segments: Vec<String>,
}

fn collect_react_nextjs_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports = collect_js_imports(content);
    let mut facts = Vec::new();
    facts.extend(collect_react_router_route_references(
        language, tree, file_path, content, &imports,
    ));
    facts.extend(collect_react_router_route_definitions(
        language, tree, file_path, content, &imports,
    ));
    facts.extend(collect_nextjs_route_references(
        language, tree, file_path, content, &imports,
    ));
    if let Some(fact) = nextjs_file_route_fact(language, file_path, content) {
        facts.push(fact);
    }
    if let Some(fact) = nuxt_file_route_fact(language, file_path, content) {
        facts.push(fact);
    }
    facts
}

fn collect_js_imports(content: &str) -> JsImportIndex {
    let mut imports = JsImportIndex::default();
    let mut cursor = 0;

    while cursor < content.len() {
        let Some(relative_import) = content[cursor..].find("import") else {
            break;
        };
        let import_start = cursor + relative_import;
        cursor = import_start + "import".len();
        if !is_identifier_boundary(content, import_start, "import".len()) {
            continue;
        }

        let statement_end = js_import_statement_end(content, import_start);
        let Some(statement) = content.get(import_start..statement_end) else {
            continue;
        };
        cursor = statement_end;

        let Some(source) = parse_import_source(statement) else {
            continue;
        };
        match source.as_str() {
            "react-router" | "react-router-dom" | "@remix-run/react" => {
                for (imported, local) in parse_named_imports(statement) {
                    match imported.as_str() {
                        "Link" | "NavLink" => {
                            imports.react_router_links.insert(local, source.clone());
                        }
                        "Route" => {
                            imports.react_router_routes.insert(local, source.clone());
                        }
                        "createBrowserRouter" | "useRoutes" | "createRoutesFromElements" => {
                            imports
                                .react_router_route_apis
                                .insert(local, source.clone());
                        }
                        _ => {}
                    }
                }
            }
            "next/link" => {
                if let Some(local) = parse_default_import(statement) {
                    imports.next_links.insert(local, source.clone());
                }
                for (imported, local) in parse_named_imports(statement) {
                    if imported == "Link" {
                        imports.next_links.insert(local, source.clone());
                    }
                }
            }
            _ => {}
        }
    }

    imports
}

fn js_import_statement_end(content: &str, import_start: usize) -> usize {
    let bytes = content.as_bytes();
    let mut cursor = import_start;
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'{' {
            brace_depth += 1;
        } else if byte == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
        } else if byte == b'(' {
            paren_depth += 1;
        } else if byte == b')' {
            paren_depth = paren_depth.saturating_sub(1);
        } else if byte == b'[' {
            bracket_depth += 1;
        } else if byte == b']' {
            bracket_depth = bracket_depth.saturating_sub(1);
        } else if byte == b';' && brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 {
            return cursor + 1;
        } else if byte == b'\n' && brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 {
            return cursor;
        }
        cursor += 1;
    }

    content.len()
}

fn parse_import_source(statement: &str) -> Option<String> {
    let from_start = statement.rfind("from")?;
    if !is_identifier_boundary(statement, from_start, "from".len()) {
        return None;
    }
    let source_start =
        skip_ascii_whitespace_until(statement, from_start + "from".len(), statement.len());
    let (source, source_end) = parse_js_string_literal(statement, source_start)?;
    (source_end <= statement.len()).then_some(source)
}

fn parse_named_imports(statement: &str) -> Vec<(String, String)> {
    let Some(open_brace) = statement.find('{') else {
        return Vec::new();
    };
    let Some(close_brace) = find_matching_brace(statement, open_brace, statement.len()) else {
        return Vec::new();
    };
    let Some(import_list) = statement.get(open_brace + 1..close_brace) else {
        return Vec::new();
    };

    import_list
        .split(',')
        .filter_map(|entry| {
            let entry = entry.trim().trim_start_matches("type ").trim();
            if entry.is_empty() {
                return None;
            }
            let mut parts = entry.split_whitespace();
            let imported = parts.next()?.trim().to_string();
            let local = if parts.next() == Some("as") {
                parts.next()?.trim().to_string()
            } else {
                imported.clone()
            };
            Some((imported, local))
        })
        .collect()
}

fn parse_default_import(statement: &str) -> Option<String> {
    let after_import = skip_ascii_whitespace_until(statement, "import".len(), statement.len());
    if matches!(
        statement.as_bytes().get(after_import),
        Some(b'{') | Some(b'*')
    ) {
        return None;
    }
    parse_js_identifier(statement, after_import, statement.len()).map(|(identifier, _)| identifier)
}

fn collect_react_router_route_references(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut cursor = 0;

    while cursor < content.len() {
        let Some((tag_start, tag_end, tag_name)) = next_markup_tag(content, cursor, content.len())
        else {
            break;
        };
        cursor = tag_end + 1;
        if is_ignored_syntax_range(tree, tag_start, tag_end + 1) {
            continue;
        }
        let Some(import_source) = imports.react_router_links.get(tag_name) else {
            continue;
        };
        let Some((target_path, span)) =
            jsx_string_literal_attribute(content, tag_start, tag_end, "to")
                .filter(|(value, _)| is_static_react_route_path(value))
        else {
            continue;
        };

        let mut metadata = base_metadata("frontend_navigation");
        insert_string(&mut metadata, "framework", "react");
        insert_string(&mut metadata, "library", "react_router");
        insert_string(&mut metadata, "target_path", &target_path);
        insert_string(&mut metadata, "attribute_name", "to");
        insert_string(&mut metadata, "component_name", tag_name);
        insert_string(&mut metadata, "import_source", import_source);
        insert_string(&mut metadata, "route_source", "string_literal");
        insert_string(&mut metadata, "source_kind", "react_router_link");

        facts.push(fact_for_span(
            file_path,
            language,
            REACT_ROUTE_REFERENCE_PATTERN_ID,
            "route_reference",
            "jsx_attribute",
            span,
            metadata,
        ));
    }

    facts
}

fn collect_react_router_route_definitions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    let mut facts =
        collect_react_router_jsx_route_definitions(language, tree, file_path, content, imports);
    facts.extend(collect_react_router_route_object_definitions(
        language, tree, file_path, content, imports,
    ));
    facts
}

fn collect_react_router_jsx_route_definitions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut cursor = 0;

    while cursor < content.len() {
        let Some((tag_start, tag_end, tag_name)) = next_markup_tag(content, cursor, content.len())
        else {
            break;
        };
        cursor = tag_end + 1;
        if is_ignored_syntax_range(tree, tag_start, tag_end + 1) {
            continue;
        }
        if !imports.react_router_routes.contains_key(tag_name) {
            continue;
        }

        let path = jsx_string_literal_attribute(content, tag_start, tag_end, "path")
            .filter(|(value, _)| is_static_react_route_path(value));
        let index_route = jsx_boolean_attribute(content, tag_start, tag_end, "index");
        if path.is_none() && !index_route {
            continue;
        }
        let Some(span) = NormalizedSpan::from_content_range(content, tag_start, tag_end + 1) else {
            continue;
        };
        let route_component =
            jsx_identifier_expression_attribute(content, tag_start, tag_end, "Component").or_else(
                || jsx_element_component_attribute(content, tag_start, tag_end, "element"),
            );

        facts.push(react_route_definition_fact(
            file_path,
            language,
            ReactRouteDefinitionFact {
                source_kind: "jsx_route",
                route_path: path.map(|(value, _)| value),
                index_route,
                route_component,
                route_id: None,
                span,
                node_kind: "jsx_element",
            },
        ));
    }

    facts
}

fn collect_react_router_route_object_definitions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    if imports.react_router_route_apis.is_empty() {
        return Vec::new();
    }

    let mut facts = Vec::new();
    for (range_start, range_end) in react_router_route_api_argument_ranges(tree, content, imports) {
        let mut cursor = range_start;
        while cursor < range_end {
            let Some(relative_path_start) = content[cursor..range_end].find("path") else {
                break;
            };
            let path_start = cursor + relative_path_start;
            cursor = path_start + "path".len();
            if !is_identifier_boundary(content, path_start, "path".len()) {
                continue;
            }
            if is_ignored_syntax_range(tree, path_start, cursor) {
                continue;
            }
            let colon = skip_ascii_whitespace_until(content, cursor, range_end);
            if content.as_bytes().get(colon) != Some(&b':') {
                continue;
            }
            let value_start = skip_ascii_whitespace_until(content, colon + 1, range_end);
            let Some((route_path, path_end)) = parse_js_string_literal(content, value_start)
                .filter(|(value, _)| is_static_react_route_path(value))
            else {
                continue;
            };
            let Some((span_start, span_end)) =
                find_enclosing_object_range(content, range_start, range_end, path_start)
            else {
                continue;
            };
            let Some(span) = NormalizedSpan::from_content_range(content, span_start, span_end)
            else {
                continue;
            };
            if path_end > span_end {
                continue;
            }

            facts.push(react_route_definition_fact(
                file_path,
                language,
                ReactRouteDefinitionFact {
                    source_kind: "route_object",
                    route_path: Some(route_path),
                    index_route: false,
                    route_component: react_route_object_component_name(
                        content, span_start, span_end,
                    ),
                    route_id: parse_object_string_property(content, span_start, span_end, "id"),
                    span,
                    node_kind: "object",
                },
            ));
        }

        let mut cursor = range_start;
        while cursor < range_end {
            let Some(relative_index_start) = content[cursor..range_end].find("index") else {
                break;
            };
            let index_start = cursor + relative_index_start;
            cursor = index_start + "index".len();
            if !is_identifier_boundary(content, index_start, "index".len()) {
                continue;
            }
            if is_ignored_syntax_range(tree, index_start, cursor) {
                continue;
            }
            let colon = skip_ascii_whitespace_until(content, cursor, range_end);
            if content.as_bytes().get(colon) != Some(&b':') {
                continue;
            }
            let value_start = skip_ascii_whitespace_until(content, colon + 1, range_end);
            if !content
                .get(value_start..)
                .is_some_and(|remaining| remaining.starts_with("true"))
            {
                continue;
            }
            let Some((span_start, span_end)) =
                find_enclosing_object_range(content, range_start, range_end, index_start)
            else {
                continue;
            };
            if parse_object_string_property(content, span_start, span_end, "path").is_some() {
                continue;
            }
            let Some(span) = NormalizedSpan::from_content_range(content, span_start, span_end)
            else {
                continue;
            };
            facts.push(react_route_definition_fact(
                file_path,
                language,
                ReactRouteDefinitionFact {
                    source_kind: "route_object",
                    route_path: None,
                    index_route: true,
                    route_component: react_route_object_component_name(
                        content, span_start, span_end,
                    ),
                    route_id: parse_object_string_property(content, span_start, span_end, "id"),
                    span,
                    node_kind: "object",
                },
            ));
        }
    }

    facts
}

fn react_router_route_api_argument_ranges(
    tree: &Tree,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for api_name in imports.react_router_route_apis.keys() {
        let mut cursor = 0;
        while cursor < content.len() {
            let Some(relative_start) = content[cursor..].find(api_name) else {
                break;
            };
            let api_start = cursor + relative_start;
            cursor = api_start + api_name.len();
            if !is_identifier_boundary(content, api_start, api_name.len()) {
                continue;
            }
            if is_ignored_syntax_range(tree, api_start, cursor) {
                continue;
            }
            let open_paren = skip_ascii_whitespace_until(content, cursor, content.len());
            if content.as_bytes().get(open_paren) != Some(&b'(') {
                continue;
            }
            let Some(close_paren) = find_matching_paren(content, open_paren, content.len()) else {
                continue;
            };
            let first_arg_start = skip_ascii_whitespace_until(content, open_paren + 1, close_paren);
            let first_arg_end = find_top_level_comma_or_end(content, first_arg_start, close_paren);
            if let Some((identifier, identifier_end)) =
                parse_js_identifier(content, first_arg_start, first_arg_end)
            {
                let trailing = skip_ascii_whitespace_until(content, identifier_end, first_arg_end);
                if trailing == first_arg_end
                    && let Some(range) = find_js_array_initializer_range(content, &identifier)
                {
                    ranges.push(range);
                    continue;
                }
            }
            ranges.push((first_arg_start, first_arg_end));
        }
    }
    ranges.sort_unstable();
    ranges.dedup();
    ranges
}

struct ReactRouteDefinitionFact<'a> {
    source_kind: &'a str,
    route_path: Option<String>,
    index_route: bool,
    route_component: Option<String>,
    route_id: Option<String>,
    span: NormalizedSpan,
    node_kind: &'a str,
}

fn react_route_definition_fact(
    file_path: &str,
    language: &str,
    fact: ReactRouteDefinitionFact<'_>,
) -> StructuralFact {
    let mut metadata = base_metadata("frontend_navigation");
    insert_string(&mut metadata, "framework", "react");
    insert_string(&mut metadata, "library", "react_router");
    insert_string(&mut metadata, "source_kind", fact.source_kind);
    if let Some(route_path) = fact.route_path {
        insert_string(&mut metadata, "route_path", &route_path);
        insert_string(&mut metadata, "route_source", "string_literal");
    } else if fact.index_route {
        insert_string(&mut metadata, "route_source", "index_route");
    }
    if fact.index_route {
        metadata.insert("index_route".to_string(), Value::Bool(true));
    }
    if let Some(route_component) = fact.route_component {
        insert_string(&mut metadata, "route_component", &route_component);
    }
    if let Some(route_id) = fact.route_id {
        insert_string(&mut metadata, "route_id", &route_id);
    }

    fact_for_span(
        file_path,
        language,
        REACT_ROUTE_DEFINITION_PATTERN_ID,
        "route_definition",
        fact.node_kind,
        fact.span,
        metadata,
    )
}

fn react_route_object_component_name(content: &str, start: usize, end: usize) -> Option<String> {
    parse_object_identifier_property(content, start, end, "Component")
        .or_else(|| parse_object_jsx_element_property_component(content, start, end, "element"))
}

fn parse_object_jsx_element_property_component(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<String> {
    let value_start = find_object_property_value_start(content, start, end, property_name)?;
    parse_jsx_element_component_at(content, value_start, end)
}

fn collect_nextjs_route_references(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut cursor = 0;

    while cursor < content.len() {
        let Some((tag_start, tag_end, tag_name)) = next_markup_tag(content, cursor, content.len())
        else {
            break;
        };
        cursor = tag_end + 1;
        if is_ignored_syntax_range(tree, tag_start, tag_end + 1) {
            continue;
        }
        let Some(import_source) = imports.next_links.get(tag_name) else {
            continue;
        };
        let href = jsx_string_literal_attribute(content, tag_start, tag_end, "href")
            .filter(|(value, _)| is_static_route_path(value))
            .map(|(value, span)| (value, "string_literal", span))
            .or_else(|| {
                jsx_object_pathname_attribute(content, tag_start, tag_end, "href")
                    .filter(|(value, _)| is_static_route_path(value))
                    .map(|(value, span)| (value, "object_pathname_literal", span))
            });
        let Some((target_path, route_source, span)) = href else {
            continue;
        };

        let mut metadata = base_metadata("frontend_navigation");
        insert_string(&mut metadata, "framework", "nextjs");
        insert_string(&mut metadata, "target_path", &target_path);
        insert_string(&mut metadata, "attribute_name", "href");
        insert_string(&mut metadata, "component_name", tag_name);
        insert_string(&mut metadata, "import_source", import_source);
        insert_string(&mut metadata, "route_source", route_source);
        insert_string(&mut metadata, "source_kind", "next_link");

        facts.push(fact_for_span(
            file_path,
            language,
            NEXTJS_ROUTE_REFERENCE_PATTERN_ID,
            "route_reference",
            "jsx_attribute",
            span,
            metadata,
        ));
    }

    facts
}

fn nextjs_file_route_fact(
    language: &str,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let route = nextjs_file_route(file_path)?;
    if route.router == "pages" && has_nuxt_page_signal(content) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, 0, content.len())?;
    let mut metadata = base_metadata("frontend_navigation");
    insert_string(&mut metadata, "framework", "nextjs");
    insert_string(&mut metadata, "router", route.router);
    insert_string(&mut metadata, "file_convention", "page");
    insert_string(&mut metadata, "route_path", &route.route_path);
    insert_string(&mut metadata, "source_kind", "nextjs_file_route");
    if let Some(normalized) = route.normalized_route_template {
        insert_string(&mut metadata, "normalized_route_template", &normalized);
    }
    if !route.dynamic_segments.is_empty() {
        insert_string_array(&mut metadata, "dynamic_segments", route.dynamic_segments);
    }
    if !route.route_group_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "route_group_segments",
            route.route_group_segments,
        );
    }

    Some(fact_for_span(
        file_path,
        language,
        NEXTJS_FILE_ROUTE_PATTERN_ID,
        "file_route",
        "file",
        span,
        metadata,
    ))
}

fn nuxt_file_route_fact(language: &str, file_path: &str, content: &str) -> Option<StructuralFact> {
    let route = nuxt_file_route(file_path)?;
    if route.router == "pages"
        && is_non_vue_file_path(file_path)
        && !has_nuxt_page_signal(content)
        && !has_nuxt_app_pages_route(file_path)
    {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, 0, content.len())?;
    let mut metadata = base_metadata("frontend_navigation");
    insert_string(&mut metadata, "framework", "nuxt");
    insert_string(&mut metadata, "router", route.router);
    insert_string(&mut metadata, "file_convention", "page");
    insert_string(&mut metadata, "route_path", &route.route_path);
    insert_string(&mut metadata, "source_kind", "nuxt_file_route");
    if let Some(normalized) = route.normalized_route_template {
        insert_string(&mut metadata, "normalized_route_template", &normalized);
    }
    if !route.dynamic_segments.is_empty() {
        insert_string_array(&mut metadata, "dynamic_segments", route.dynamic_segments);
    }
    if !route.route_group_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "route_group_segments",
            route.route_group_segments,
        );
    }

    Some(fact_for_span(
        file_path,
        language,
        NUXT_FILE_ROUTE_PATTERN_ID,
        "file_route",
        "file",
        span,
        metadata,
    ))
}

fn nextjs_file_route(file_path: &str) -> Option<NextFileRoute> {
    let normalized = file_path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if let Some(route) = segments
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, segment)| match *segment {
            "app" => nextjs_app_file_route(&segments, index),
            "pages" if segments.get(index.wrapping_sub(1)) != Some(&"app") => {
                nextjs_pages_file_route(&segments, index)
            }
            _ => None,
        })
    {
        return Some(route);
    }
    None
}

fn nuxt_file_route(file_path: &str) -> Option<NextFileRoute> {
    let normalized = file_path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if let Some(app_index) = segments
        .windows(2)
        .enumerate()
        .rev()
        .find_map(|(index, window)| (window == ["app", "pages"]).then_some(index))
    {
        return nuxt_pages_file_route(&segments, app_index + 1);
    }
    if let Some(pages_index) = segments
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, segment)| (*segment == "pages").then_some(index))
    {
        return nuxt_pages_file_route(&segments, pages_index);
    }
    None
}

fn nextjs_app_file_route(segments: &[&str], app_index: usize) -> Option<NextFileRoute> {
    let file_name = *segments.last()?;
    let (stem, extension) = split_file_name(file_name)?;
    if stem != "page" || !is_javascript_like_extension(extension) {
        return None;
    }

    let mut route_segments = Vec::new();
    let mut route_group_segments = Vec::new();
    for segment in &segments[app_index + 1..segments.len().saturating_sub(1)] {
        if segment.starts_with('(') && segment.ends_with(')') && segment.len() > 2 {
            route_group_segments.push(segment[1..segment.len() - 1].to_string());
        } else {
            route_segments.push((*segment).to_string());
        }
    }

    let (route_path, normalized_route_template, dynamic_segments) =
        nextjs_route_path_metadata(&route_segments);
    Some(NextFileRoute {
        router: "app",
        route_path,
        normalized_route_template,
        dynamic_segments,
        route_group_segments,
    })
}

fn nextjs_pages_file_route(segments: &[&str], pages_index: usize) -> Option<NextFileRoute> {
    let file_name = *segments.last()?;
    let (stem, extension) = split_file_name(file_name)?;
    if !is_javascript_like_extension(extension) || stem.starts_with('_') {
        return None;
    }
    if segments.get(pages_index + 1) == Some(&"api") {
        return None;
    }

    let mut route_segments = segments[pages_index + 1..segments.len().saturating_sub(1)]
        .iter()
        .map(|segment| (*segment).to_string())
        .collect::<Vec<_>>();
    if stem != "index" {
        route_segments.push(stem.to_string());
    }

    let (route_path, normalized_route_template, dynamic_segments) =
        nextjs_route_path_metadata(&route_segments);
    Some(NextFileRoute {
        router: "pages",
        route_path,
        normalized_route_template,
        dynamic_segments,
        route_group_segments: Vec::new(),
    })
}

fn nuxt_pages_file_route(segments: &[&str], pages_index: usize) -> Option<NextFileRoute> {
    let file_name = *segments.last()?;
    let (stem, extension) = split_file_name(file_name)?;
    if !is_nuxt_page_extension(extension) || stem.starts_with('_') || stem.contains('@') {
        return None;
    }
    if segments.get(pages_index + 1) == Some(&"api") {
        return None;
    }

    let mut route_segments = Vec::new();
    let mut route_group_segments = Vec::new();
    for segment in &segments[pages_index + 1..segments.len().saturating_sub(1)] {
        if segment.starts_with('(') && segment.ends_with(')') && segment.len() > 2 {
            route_group_segments.push(segment[1..segment.len() - 1].to_string());
        } else {
            route_segments.push((*segment).to_string());
        }
    }
    if stem != "index" {
        route_segments.push(stem.to_string());
    }

    let (route_path, normalized_route_template, dynamic_segments) =
        nextjs_route_path_metadata(&route_segments);
    Some(NextFileRoute {
        router: "pages",
        route_path,
        normalized_route_template,
        dynamic_segments,
        route_group_segments,
    })
}

fn nextjs_route_path_metadata(route_segments: &[String]) -> (String, Option<String>, Vec<String>) {
    let mut normalized_segments = Vec::new();
    let mut dynamic_segments = Vec::new();
    let mut has_dynamic = false;

    for segment in route_segments {
        if let Some((name, normalized)) = nextjs_dynamic_segment_metadata(segment) {
            has_dynamic = true;
            dynamic_segments.push(name);
            normalized_segments.push(normalized);
        } else {
            normalized_segments.push(segment.clone());
        }
    }

    let route_path = if route_segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", route_segments.join("/"))
    };
    let normalized_route_template = has_dynamic.then(|| {
        if normalized_segments.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", normalized_segments.join("/"))
        }
    });

    (route_path, normalized_route_template, dynamic_segments)
}

fn nextjs_dynamic_segment_metadata(segment: &str) -> Option<(String, String)> {
    if segment.starts_with("[[...") && segment.ends_with("]]") {
        let name = segment
            .trim_start_matches("[[...")
            .trim_end_matches("]]")
            .to_string();
        return Some((name.clone(), format!(":{name}*?")));
    }
    if segment.starts_with("[...") && segment.ends_with(']') {
        let name = segment
            .trim_start_matches("[...")
            .trim_end_matches(']')
            .to_string();
        return Some((name.clone(), format!(":{name}*")));
    }
    if segment.starts_with('[') && segment.ends_with(']') {
        let name = segment
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        return Some((name.clone(), format!(":{name}")));
    }
    None
}

fn split_file_name(file_name: &str) -> Option<(&str, &str)> {
    let dot = file_name.rfind('.')?;
    Some((&file_name[..dot], &file_name[dot + 1..]))
}

fn is_javascript_like_extension(extension: &str) -> bool {
    matches!(extension, "js" | "jsx" | "ts" | "tsx")
}

fn is_nuxt_page_extension(extension: &str) -> bool {
    matches!(extension, "vue" | "js" | "jsx" | "mjs" | "ts" | "tsx")
}

fn is_non_vue_file_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    let Some(file_name) = normalized.split('/').rfind(|segment| !segment.is_empty()) else {
        return false;
    };
    split_file_name(file_name).is_some_and(|(_, extension)| extension != "vue")
}

fn has_nuxt_app_pages_route(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .windows(2)
        .any(|window| window == ["app", "pages"])
}

fn has_nuxt_page_signal(content: &str) -> bool {
    [
        "defineComponent",
        "defineNuxtComponent",
        "definePageMeta",
        "defineNuxtRouteMiddleware",
        "useNuxtApp",
        "#app",
        "#imports",
        "nuxt/app",
    ]
    .iter()
    .any(|signal| content.contains(signal))
}

fn next_markup_tag(content: &str, start: usize, end: usize) -> Option<(usize, usize, &str)> {
    let mut cursor = start;
    while cursor < end {
        let relative_tag_start = content.get(cursor..end)?.find('<')?;
        let tag_start = cursor + relative_tag_start;
        let tag_end = find_tag_end(content, tag_start).filter(|tag_end| *tag_end <= end)?;
        cursor = tag_end + 1;
        if !is_markup_tag_start(content.as_bytes(), tag_start) {
            continue;
        }
        let Some(tag_name) = markup_tag_name(content, tag_start, tag_end) else {
            continue;
        };
        return Some((tag_start, tag_end, tag_name));
    }
    None
}

fn jsx_string_literal_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<(String, NormalizedSpan)> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    let bytes = content.as_bytes();
    let value = if matches!(bytes.get(value_start), Some(b'\"') | Some(b'\'')) {
        parse_js_string_literal(content, value_start)?.0
    } else if bytes.get(value_start) == Some(&b'{') {
        let close = find_matching_brace(content, value_start, attribute.value_end)?;
        let literal_start = skip_ascii_whitespace_until(content, value_start + 1, close);
        let (value, literal_end) = parse_js_string_literal(content, literal_start)?;
        let trailing = skip_ascii_whitespace_until(content, literal_end, close);
        if trailing != close {
            return None;
        }
        value
    } else {
        return None;
    };
    Some((value, attribute.span))
}

fn jsx_object_pathname_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<(String, NormalizedSpan)> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    if content.as_bytes().get(value_start) != Some(&b'{') {
        return None;
    }
    let close = find_matching_brace(content, value_start, attribute.value_end)?;
    let object_start = skip_ascii_whitespace_until(content, value_start + 1, close);
    if content.as_bytes().get(object_start) != Some(&b'{') {
        return None;
    }
    let object_end = find_matching_brace(content, object_start, close)?;
    let value = parse_object_string_property(content, object_start, object_end + 1, "pathname")?;
    Some((value, attribute.span))
}

fn jsx_boolean_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> bool {
    find_jsx_attribute(content, tag_start, tag_end, attribute_name)
        .is_some_and(|attribute| attribute.value_start.is_none())
}

fn jsx_identifier_expression_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<String> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    if content.as_bytes().get(value_start) != Some(&b'{') {
        return None;
    }
    let close = find_matching_brace(content, value_start, attribute.value_end)?;
    let identifier_start = skip_ascii_whitespace_until(content, value_start + 1, close);
    let (identifier, identifier_end) = parse_js_identifier(content, identifier_start, close)?;
    let trailing = skip_ascii_whitespace_until(content, identifier_end, close);
    (trailing == close).then_some(identifier)
}

fn jsx_element_component_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<String> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    if content.as_bytes().get(value_start) != Some(&b'{') {
        return parse_jsx_element_component_at(content, value_start, attribute.value_end);
    }
    let close = find_matching_brace(content, value_start, attribute.value_end)?;
    let expression_start = skip_ascii_whitespace_until(content, value_start + 1, close);
    parse_jsx_element_component_at(content, expression_start, close)
}

fn parse_jsx_element_component_at(content: &str, value_start: usize, end: usize) -> Option<String> {
    if content.as_bytes().get(value_start) != Some(&b'<') {
        return None;
    }
    let component_start = value_start + 1;
    if matches!(
        content.as_bytes().get(component_start),
        Some(b'>') | Some(b'/')
    ) {
        return None;
    }
    parse_js_identifier(content, component_start, end).map(|(identifier, _)| identifier)
}

fn find_jsx_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<JsxAttributeSpan> {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
        cursor += 1;
    }

    while cursor < tag_end {
        cursor = skip_ascii_whitespace_until(content, cursor, tag_end);
        if cursor >= tag_end || bytes[cursor] == b'/' {
            cursor += 1;
            continue;
        }

        let attribute_start = cursor;
        while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == attribute_start {
            cursor += 1;
            continue;
        }

        let attribute_end = cursor;
        let Some(name) = content.get(attribute_start..attribute_end) else {
            continue;
        };
        let after_name = skip_ascii_whitespace_until(content, cursor, tag_end);
        if content.as_bytes().get(after_name) != Some(&b'=') {
            if name != attribute_name {
                continue;
            }
            let span = NormalizedSpan::from_content_range(content, attribute_start, cursor)?;
            return Some(JsxAttributeSpan {
                value_start: None,
                value_end: cursor,
                span,
            });
        }
        let value_start = skip_ascii_whitespace_until(content, after_name + 1, tag_end);
        let value_end = jsx_attribute_value_end(content, value_start, tag_end)?;
        cursor = value_end;
        if name != attribute_name {
            continue;
        }
        let span = NormalizedSpan::from_content_range(content, attribute_start, value_end)?;
        return Some(JsxAttributeSpan {
            value_start: Some(value_start),
            value_end,
            span,
        });
    }
    None
}

fn jsx_attribute_value_end(content: &str, value_start: usize, tag_end: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    match bytes.get(value_start)? {
        b'\'' | b'\"' => {
            let (_, end) = parse_js_string_literal(content, value_start)?;
            Some(end)
        }
        b'{' => find_matching_brace(content, value_start, tag_end).map(|end| end + 1),
        _ => {
            let mut cursor = value_start;
            while cursor < tag_end
                && !bytes[cursor].is_ascii_whitespace()
                && !matches!(bytes[cursor], b'/' | b'>')
            {
                cursor += 1;
            }
            Some(cursor)
        }
    }
}

fn is_static_react_route_path(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !value.starts_with("//") && !value.contains("://")
}

fn markup_tag_name(content: &str, tag_start: usize, tag_end: usize) -> Option<&str> {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    let name_start = cursor;
    while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
        cursor += 1;
    }
    (cursor > name_start)
        .then(|| content.get(name_start..cursor))
        .flatten()
}

fn is_static_route_path(value: &str) -> bool {
    value.trim().starts_with('/')
}

fn is_nuxt_route_path(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('/') && !value.starts_with("//")
}

fn is_nuxt_link_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "nuxtlink" | "nuxt-link"
    )
}

fn is_nuxt_external_attribute(attribute_name: &str) -> bool {
    matches!(attribute_name, "external" | ":external" | "v-bind:external")
}

fn collect_vue_static_imports(content: &str, section: &VueSectionSpan) -> HashMap<String, String> {
    let mut imports = HashMap::new();
    let Some(section_text) = content.get(section.content_start..section.content_end) else {
        return imports;
    };

    for line in section_text.lines() {
        if let Some((binding, specifier)) = parse_vue_static_import_line(line.trim()) {
            imports.insert(binding, specifier);
        }
    }

    imports
}

fn parse_vue_static_import_line(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("import ")?;
    let (binding, specifier) = rest.split_once(" from ")?;
    let binding = binding.trim();
    if !is_js_identifier(binding) {
        return None;
    }
    let specifier = specifier.trim().trim_end_matches(';').trim();
    let (specifier, _) = parse_js_string_literal(specifier, 0)?;
    Some((binding.to_string(), specifier))
}

fn parse_object_string_property(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<String> {
    let value_start = find_object_property_value_start(content, start, end, property_name)?;
    let (value, value_end) = parse_js_string_literal(content, value_start)?;
    (value_end <= end).then_some(value)
}

fn parse_object_identifier_property(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<String> {
    let value_start = find_object_property_value_start(content, start, end, property_name)?;
    let (identifier, _) = parse_js_identifier(content, value_start, end)?;
    Some(identifier)
}

fn find_object_property_value_start(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<usize> {
    let mut cursor = start;
    while cursor < end {
        let Some(relative_start) = content[cursor..end].find(property_name) else {
            break;
        };
        let property_start = cursor + relative_start;
        cursor = property_start + property_name.len();
        if !is_identifier_boundary(content, property_start, property_name.len()) {
            continue;
        }
        let colon = skip_ascii_whitespace_until(content, cursor, end);
        if content.as_bytes().get(colon) != Some(&b':') {
            continue;
        }
        return Some(skip_ascii_whitespace_until(content, colon + 1, end));
    }
    None
}

fn parse_js_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    let quote = bytes
        .get(start)
        .copied()
        .filter(|byte| matches!(*byte, b'\'' | b'"'))?;
    let mut cursor = start + 1;
    let mut value = String::new();

    while cursor < content.len() {
        let byte = bytes[cursor];
        if byte == b'\\' {
            let escaped_start = cursor + 1;
            let escaped = content.get(escaped_start..)?.chars().next()?;
            value.push(escaped);
            cursor = escaped_start + escaped.len_utf8();
        } else if byte == quote {
            return Some((value, cursor + 1));
        } else {
            let ch = content.get(cursor..)?.chars().next()?;
            value.push(ch);
            cursor += ch.len_utf8();
        }
    }

    None
}

fn parse_js_identifier(content: &str, start: usize, end: usize) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    let first = *bytes.get(start)?;
    if !is_js_identifier_start_byte(first) {
        return None;
    }
    let mut cursor = start + 1;
    while cursor < end
        && bytes
            .get(cursor)
            .is_some_and(|byte| is_js_identifier_byte(*byte))
    {
        cursor += 1;
    }
    Some((content.get(start..cursor)?.to_string(), cursor))
}

fn is_js_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    is_js_identifier_start_byte(first) && bytes.all(is_js_identifier_byte)
}

fn is_identifier_boundary(content: &str, start: usize, len: usize) -> bool {
    let bytes = content.as_bytes();
    let before = start.checked_sub(1).and_then(|index| bytes.get(index));
    let after = bytes.get(start + len);
    !before.is_some_and(|byte| is_js_identifier_byte(*byte))
        && !after.is_some_and(|byte| is_js_identifier_byte(*byte))
}

fn is_js_identifier_start_byte(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_js_identifier_byte(byte: u8) -> bool {
    is_js_identifier_start_byte(byte) || byte.is_ascii_digit()
}

fn is_ignored_syntax_range(tree: &Tree, start_byte: usize, end_byte: usize) -> bool {
    smallest_node_covering_range(tree.root_node(), start_byte, end_byte)
        .is_some_and(|node| node_or_parent_is_comment_or_string(node))
}

fn smallest_node_covering_range<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
) -> Option<Node<'tree>> {
    smallest_node_covering_range_at_depth(node, start_byte, end_byte, 0)
}

fn smallest_node_covering_range_at_depth<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
    depth: u32,
) -> Option<Node<'tree>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.start_byte() > start_byte || node.end_byte() < end_byte {
        return None;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return Some(node);
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) =
            smallest_node_covering_range_at_depth(child, start_byte, end_byte, child_depth)
        {
            return Some(found);
        }
    }

    Some(node)
}

fn node_or_parent_is_comment_or_string(mut node: Node<'_>) -> bool {
    loop {
        if is_comment_or_string_node(node.kind()) {
            return true;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}

fn is_comment_or_string_node(node_kind: &str) -> bool {
    node_kind.contains("comment") || node_kind.contains("string")
}

fn find_enclosing_object_range(
    content: &str,
    start: usize,
    end: usize,
    position: usize,
) -> Option<(usize, usize)> {
    let object_start = content.get(start..position)?.rfind('{')? + start;
    let object_end = find_matching_brace(content, object_start, end)?;
    Some((object_start, object_end + 1))
}

fn find_matching_brace(content: &str, open_brace: usize, end: usize) -> Option<usize> {
    if content.as_bytes().get(open_brace) != Some(&b'{') {
        return None;
    }
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut cursor = open_brace;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

fn find_matching_paren(content: &str, open_paren: usize, end: usize) -> Option<usize> {
    if content.as_bytes().get(open_paren) != Some(&b'(') {
        return None;
    }
    let bytes = content.as_bytes();
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut cursor = open_paren;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
        } else if byte == b'{' {
            brace_depth += 1;
        } else if byte == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
        } else if byte == b'[' {
            bracket_depth += 1;
        } else if byte == b']' {
            bracket_depth = bracket_depth.saturating_sub(1);
        } else if byte == b'(' {
            paren_depth += 1;
        } else if byte == b')' {
            paren_depth = paren_depth.saturating_sub(1);
            if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

fn find_matching_bracket(content: &str, open_bracket: usize, end: usize) -> Option<usize> {
    if content.as_bytes().get(open_bracket) != Some(&b'[') {
        return None;
    }
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut cursor = open_bracket;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
        } else if byte == b'[' {
            depth += 1;
        } else if byte == b']' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

fn find_top_level_comma_or_end(content: &str, start: usize, end: usize) -> usize {
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = content.as_bytes()[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
        } else {
            match byte {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b',' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                    return cursor;
                }
                _ => {}
            }
        }
        cursor += 1;
    }

    end
}

fn find_js_array_initializer_range(content: &str, identifier: &str) -> Option<(usize, usize)> {
    find_js_array_initializer_range_in(content, identifier, 0, content.len())
}

fn find_js_array_initializer_range_in(
    content: &str,
    identifier: &str,
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let mut cursor = start;
    while cursor < end {
        let Some(relative_start) = content[cursor..end].find(identifier) else {
            break;
        };
        let identifier_start = cursor + relative_start;
        cursor = identifier_start + identifier.len();
        if !is_identifier_boundary(content, identifier_start, identifier.len()) {
            continue;
        }
        let equals = skip_ascii_whitespace_until(content, cursor, end);
        if content.as_bytes().get(equals) != Some(&b'=') {
            continue;
        }
        let array_start = skip_ascii_whitespace_until(content, equals + 1, end);
        if content.as_bytes().get(array_start) != Some(&b'[') {
            continue;
        }
        let array_end = find_matching_bracket(content, array_start, end)?;
        return Some((array_start, array_end + 1));
    }
    None
}

fn fact_for_node(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node: Node<'_>,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    fact_for_span(
        file_path,
        language,
        pattern_id,
        capture_name,
        node.kind(),
        NormalizedSpan::from_node(&node),
        metadata,
    )
}

fn fact_for_span(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node_kind: &str,
    span: NormalizedSpan,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    StructuralFact {
        id: stable_location_id(file_path, &format!("{pattern_id}:{capture_name}"), span),
        file_path: file_path.to_string(),
        language: language.to_string(),
        pattern_id: pattern_id.to_string(),
        capture_name: capture_name.to_string(),
        node_kind: node_kind.to_string(),
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    }
}

fn base_metadata(query_family: &str) -> HashMap<String, Value> {
    HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String(query_family.to_string()),
        ),
    ])
}

fn insert_string(metadata: &mut HashMap<String, Value>, key: &str, value: &str) {
    metadata.insert(key.to_string(), Value::String(value.to_string()));
}

fn insert_string_array(metadata: &mut HashMap<String, Value>, key: &str, values: Vec<String>) {
    metadata.insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
}

fn attach_containing_symbols(facts: &mut [StructuralFact], symbols: &[Symbol]) {
    for fact in facts {
        fact.containing_symbol_id = symbols
            .iter()
            .filter(|symbol| {
                symbol.start_byte <= fact.start_byte && symbol.end_byte >= fact.end_byte
            })
            .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
            .map(|symbol| symbol.id.clone());
    }
}

fn child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

fn css_selector_kind(selector: &str) -> &'static str {
    let selector = selector.trim();
    if selector.contains(',') {
        "selector_list"
    } else if selector.starts_with('.') {
        "class"
    } else if selector.starts_with('#') {
        "id"
    } else if selector.starts_with(':') {
        "pseudo"
    } else {
        "compound"
    }
}

fn count_css_declarations(node: Node<'_>) -> usize {
    count_css_declarations_at_depth(node, 0)
}

fn count_css_declarations_at_depth(node: Node<'_>, depth: u32) -> usize {
    if !should_visit_tree_depth(depth) {
        return 0;
    }
    let mut count = usize::from(node.kind() == "declaration");
    let Some(child_depth) = child_tree_depth(depth) else {
        return count;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_css_declarations_at_depth(child, child_depth);
    }
    count
}

fn css_at_rule_prelude<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix(keyword)?.trim();
    let prelude = rest.split('{').next().unwrap_or(rest).trim();
    (!prelude.is_empty()).then_some(prelude)
}

#[derive(Debug)]
struct VueSectionSpan {
    section_type: &'static str,
    lang: Option<String>,
    setup: bool,
    scoped: bool,
    start_span: NormalizedSpan,
    content_start: usize,
    content_end: usize,
}

fn scan_vue_sections(content: &str) -> Vec<VueSectionSpan> {
    let mut sections = Vec::new();
    let mut cursor = 0usize;

    while let Some((tag_start, section_type)) = next_vue_section_start(content, cursor) {
        let Some(open_tag_end) = find_tag_end(content, tag_start) else {
            break;
        };
        let close_tag = format!("</{section_type}>");
        let content_start = open_tag_end + 1;
        let Some(close_relative) = content[content_start..].find(&close_tag) else {
            cursor = content_start;
            continue;
        };
        let content_end = content_start + close_relative;
        let tag_end = content_end + close_tag.len();
        let Some(span) = NormalizedSpan::from_content_range(content, tag_start, tag_end) else {
            cursor = tag_end;
            continue;
        };

        let attrs = content.get(tag_start..=open_tag_end).unwrap_or_default();
        sections.push(VueSectionSpan {
            section_type,
            lang: parse_attr_value(attrs, "lang"),
            setup: has_boolean_attr(attrs, "setup"),
            scoped: has_boolean_attr(attrs, "scoped"),
            start_span: span,
            content_start,
            content_end,
        });
        cursor = tag_end;
    }

    sections
}

fn next_vue_section_start(content: &str, cursor: usize) -> Option<(usize, &'static str)> {
    ["template", "script", "style"]
        .into_iter()
        .filter_map(|section| {
            content[cursor..]
                .find(&format!("<{section}"))
                .map(|relative| (cursor + relative, section))
        })
        .min_by_key(|(start, _)| *start)
}

#[derive(Debug)]
struct MarkupAttribute {
    tag_name: String,
    name: String,
    value: Option<String>,
    span: NormalizedSpan,
}

struct VueRouteReference {
    source_kind: &'static str,
    target_path: String,
    expression: Option<String>,
}

fn scan_markup_attributes(content: &str, start: usize, end: usize) -> Vec<MarkupAttribute> {
    let mut attributes = Vec::new();
    let mut cursor = start;

    while cursor < end {
        let Some(relative_tag_start) = content[cursor..end].find('<') else {
            break;
        };
        let tag_start = cursor + relative_tag_start;
        let Some(tag_end) = find_tag_end(content, tag_start).filter(|tag_end| *tag_end <= end)
        else {
            break;
        };
        if is_markup_tag_start(content.as_bytes(), tag_start) {
            scan_tag_attributes(content, tag_start, tag_end, &mut attributes);
        }
        cursor = tag_end + 1;
    }

    attributes
}

fn scan_tag_attributes(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attributes: &mut Vec<MarkupAttribute>,
) {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
        cursor += 1;
    }
    let tag_name = content
        .get(tag_start + 1..cursor)
        .unwrap_or_default()
        .to_ascii_lowercase();

    while cursor < tag_end {
        cursor = skip_ascii_whitespace_until(content, cursor, tag_end);
        if cursor >= tag_end || bytes[cursor] == b'/' {
            cursor += 1;
            continue;
        }

        let name_start = cursor;
        while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == name_start {
            cursor += 1;
            continue;
        }

        let name_end = cursor;
        let mut value = None;
        let mut attr_end = name_end;
        cursor = skip_ascii_whitespace_until(content, cursor, tag_end);
        if cursor < tag_end && bytes[cursor] == b'=' {
            cursor = skip_ascii_whitespace_until(content, cursor + 1, tag_end);
            let (parsed_value, value_end) = parse_markup_attribute_value(content, cursor, tag_end);
            value = parsed_value;
            attr_end = value_end;
            cursor = value_end;
        }

        let Some(span) = NormalizedSpan::from_content_range(content, name_start, attr_end) else {
            continue;
        };
        let Some(name) = content.get(name_start..name_end) else {
            continue;
        };
        attributes.push(MarkupAttribute {
            tag_name: tag_name.clone(),
            name: name.to_string(),
            value,
            span,
        });
    }
}

fn parse_markup_attribute_value(
    content: &str,
    value_start: usize,
    tag_end: usize,
) -> (Option<String>, usize) {
    let bytes = content.as_bytes();
    let Some(quote) = bytes
        .get(value_start)
        .copied()
        .filter(|byte| matches!(*byte, b'"' | b'\''))
    else {
        let mut value_end = value_start;
        while value_end < tag_end && !bytes[value_end].is_ascii_whitespace() {
            value_end += 1;
        }
        return (
            content.get(value_start..value_end).map(ToString::to_string),
            value_end,
        );
    };

    let mut value_end = value_start + 1;
    while value_end < tag_end && bytes[value_end] != quote {
        value_end += 1;
    }
    let value = content
        .get(value_start + 1..value_end)
        .map(ToString::to_string);
    let attr_end = if value_end < tag_end {
        value_end + 1
    } else {
        value_end
    };
    (value, attr_end)
}

fn find_tag_end(content: &str, tag_start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    let mut quote = None;
    let mut brace_depth = 0usize;
    let mut escaped = false;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'{' {
            brace_depth += 1;
        } else if byte == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
        } else if byte == b'>' && brace_depth == 0 {
            return Some(cursor);
        }
        cursor += 1;
    }

    None
}

fn is_markup_tag_start(bytes: &[u8], tag_start: usize) -> bool {
    let Some(next) = bytes.get(tag_start + 1) else {
        return false;
    };
    !matches!(*next, b'!' | b'?' | b'/')
}

fn is_attr_name_byte(byte: u8) -> bool {
    !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'/' | b'>' | b'<')
}

fn skip_ascii_whitespace_until(content: &str, mut cursor: usize, end: usize) -> usize {
    let bytes = content.as_bytes();
    while cursor < end && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

fn parse_attr_value(attrs: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let prefix = format!("{name}={quote}");
        if let Some(start) = attrs.find(&prefix) {
            let value_start = start + prefix.len();
            let value_end = attrs[value_start..].find(quote)? + value_start;
            return Some(attrs[value_start..value_end].to_string());
        }
    }
    None
}

fn has_boolean_attr(attrs: &str, name: &str) -> bool {
    attrs
        .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '<' | '>' | '/'))
        .any(|part| part == name)
}

#[derive(Debug)]
struct VueDirective {
    name: &'static str,
    argument: Option<String>,
    modifiers: Vec<String>,
    shorthand: bool,
}

fn parse_vue_directive(attribute_name: &str) -> Option<VueDirective> {
    if let Some(rest) = attribute_name.strip_prefix('@') {
        let (argument, modifiers) = split_argument_and_modifiers(rest);
        return Some(VueDirective {
            name: "v-on",
            argument,
            modifiers,
            shorthand: true,
        });
    }

    if let Some(rest) = attribute_name.strip_prefix(':') {
        let (argument, modifiers) = split_argument_and_modifiers(rest);
        return Some(VueDirective {
            name: "v-bind",
            argument,
            modifiers,
            shorthand: true,
        });
    }

    let rest = attribute_name.strip_prefix("v-")?;
    let base = rest
        .find(&[':', '.'][..])
        .map(|index| &rest[..index])
        .unwrap_or(rest);
    let directive_name = match base {
        "bind" => "v-bind",
        "on" => "v-on",
        "if" => "v-if",
        "else-if" => "v-else-if",
        "else" => "v-else",
        "for" => "v-for",
        "show" => "v-show",
        "model" => "v-model",
        "slot" => "v-slot",
        "text" => "v-text",
        "html" => "v-html",
        "pre" => "v-pre",
        "once" => "v-once",
        "memo" => "v-memo",
        "cloak" => "v-cloak",
        _ => return None,
    };

    let mut argument = None;
    let mut modifiers = Vec::new();
    let tail_start = "v-".len() + base.len();
    if let Some(separator) = attribute_name.as_bytes().get(tail_start).copied() {
        let tail = &attribute_name[tail_start + 1..];
        if separator == b':' {
            let (parsed_argument, parsed_modifiers) = split_argument_and_modifiers(tail);
            argument = parsed_argument;
            modifiers = parsed_modifiers;
        } else if separator == b'.' {
            modifiers = tail
                .split('.')
                .filter(|modifier| !modifier.is_empty())
                .map(ToString::to_string)
                .collect();
        }
    }

    Some(VueDirective {
        name: directive_name,
        argument,
        modifiers,
        shorthand: false,
    })
}

fn vue_route_reference(
    attribute: &MarkupAttribute,
    directive: Option<&VueDirective>,
) -> Option<VueRouteReference> {
    if is_vue_router_link_tag(&attribute.tag_name) {
        if attribute.name == "to" {
            let target_path = attribute.value.as_deref()?.trim();
            if is_vue_route_path(target_path) {
                return Some(VueRouteReference {
                    source_kind: "router_link",
                    target_path: target_path.to_string(),
                    expression: None,
                });
            }
        }

        if is_vue_to_binding(directive) {
            let expression = attribute.value.as_deref()?.trim();
            let target_path = parse_vue_string_literal(expression)?;
            if is_vue_route_path(&target_path) {
                return Some(VueRouteReference {
                    source_kind: "router_link",
                    target_path,
                    expression: Some(expression.to_string()),
                });
            }
        }
    }

    if directive.is_some_and(|directive| directive.name == "v-on") {
        let expression = attribute.value.as_deref()?.trim();
        let target_path = parse_vue_router_navigation_literal(expression)?;
        return Some(VueRouteReference {
            source_kind: "router_navigation_expression",
            target_path,
            expression: Some(expression.to_string()),
        });
    }

    None
}

fn is_vue_router_link_tag(tag_name: &str) -> bool {
    matches!(tag_name, "router-link" | "routerlink")
}

fn is_vue_to_binding(directive: Option<&VueDirective>) -> bool {
    directive.is_some_and(|directive| {
        directive.name == "v-bind" && directive.argument.as_deref() == Some("to")
    })
}

fn parse_vue_router_navigation_literal(expression: &str) -> Option<String> {
    let open_paren = expression.find('(')?;
    let receiver = expression[..open_paren].trim();
    if !matches!(
        receiver,
        "$router.push" | "$router.replace" | "router.push" | "router.replace"
    ) {
        return None;
    }

    let close_paren = expression.rfind(')')?;
    if !expression[close_paren + 1..].trim().is_empty() {
        return None;
    }

    let target_path = parse_vue_string_literal(expression[open_paren + 1..close_paren].trim())?;
    is_vue_route_path(&target_path).then_some(target_path)
}

fn parse_vue_string_literal(expression: &str) -> Option<String> {
    let trimmed = expression.trim();
    let bytes = trimmed.as_bytes();
    let quote = *bytes.first()?;
    if !matches!(quote, b'\'' | b'"') || bytes.last().copied() != Some(quote) {
        return None;
    }
    let inner = trimmed.get(1..trimmed.len().saturating_sub(1))?;
    if inner.as_bytes().contains(&b'\\') {
        return None;
    }
    Some(inner.to_string())
}

fn is_vue_route_path(value: &str) -> bool {
    value.starts_with('/') && !value.starts_with("//")
}

fn split_argument_and_modifiers(value: &str) -> (Option<String>, Vec<String>) {
    let mut parts = value.split('.').filter(|part| !part.is_empty());
    let argument = parts.next().map(ToString::to_string);
    let modifiers = parts.map(ToString::to_string).collect();
    (argument, modifiers)
}

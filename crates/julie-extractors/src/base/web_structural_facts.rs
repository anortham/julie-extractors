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
    VUE_SFC_SECTION_PATTERN_ID,
    VUE_TEMPLATE_DIRECTIVE_PATTERN_ID,
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
        "vue" => collect_vue_structural_facts(file_path, content),
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

fn collect_vue_structural_facts(file_path: &str, content: &str) -> Vec<StructuralFact> {
    let mut facts = Vec::new();

    for section in scan_vue_sections(content) {
        facts.push(vue_section_fact(file_path, &section));

        if section.section_type == "template" {
            for attribute in
                scan_markup_attributes(content, section.content_start, section.content_end)
            {
                if let Some(directive) = parse_vue_directive(&attribute.name) {
                    facts.push(vue_template_directive_fact(
                        file_path, &attribute, directive,
                    ));
                }
            }
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
    name: String,
    value: Option<String>,
    span: NormalizedSpan,
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

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if byte == active_quote {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
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

fn split_argument_and_modifiers(value: &str) -> (Option<String>, Vec<String>) {
    let mut parts = value.split('.').filter(|part| !part.is_empty());
    let argument = parts.next().map(ToString::to_string);
    let modifiers = parts.map(ToString::to_string).collect();
    (argument, modifiers)
}

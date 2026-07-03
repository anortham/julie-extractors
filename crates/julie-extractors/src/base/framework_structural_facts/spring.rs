use tree_sitter::Tree;

use super::SPRING_REQUEST_MAPPING_PATTERN_ID;
use super::helpers::{
    base_metadata, fact_for_span, insert_string, insert_string_array, is_comment_or_string_node,
    skip_ascii_whitespace_until, smallest_node_covering_range,
};
use super::scan::{
    MaskLanguage, SourceMask, find_matching_brace_within, find_matching_paren,
    find_top_level_comma_or_end, parse_java_string_literal,
};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

pub(super) fn collect_spring_request_mappings(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains("import org.springframework.web.bind.annotation.") {
        return Vec::new();
    }

    let mask = SourceMask::new(content, MaskLanguage::Java);
    let mut facts = Vec::new();
    let mut pending_class_mapping: Option<(MappingAnnotation, usize, usize)> = None;
    let mut current_class_templates: Vec<String> = Vec::new();
    let mut offset = 0;
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        let line_offset = offset + (line.len() - line.trim_start().len());
        if trimmed.starts_with('@')
            && !mask.is_string_or_comment(line_offset)
            && let Some(mapping) = parse_mapping_annotation(content, &mask, offset, line)
        {
            let (target_start, target_end, target_kind) = next_java_declaration_line(
                content,
                offset + line.len(),
            )
            .unwrap_or((offset, offset + line.len(), DeclarationKind::Method));
            if target_kind == DeclarationKind::Class {
                pending_class_mapping = Some((mapping, target_start, target_end));
            } else {
                let templates = if mapping.templates.is_empty() {
                    if mapping.has_route_argument {
                        Vec::new()
                    } else {
                        vec!["".to_string()]
                    }
                } else {
                    mapping.templates.clone()
                };
                let class_templates: Vec<Option<&str>> = if current_class_templates.is_empty() {
                    vec![None]
                } else {
                    current_class_templates
                        .iter()
                        .map(|template| Some(template.as_str()))
                        .collect()
                };
                let verbs: Vec<Option<&str>> = if mapping.verbs.is_empty() {
                    vec![None]
                } else {
                    mapping
                        .verbs
                        .iter()
                        .map(|verb| Some(verb.as_str()))
                        .collect()
                };
                for class_template in class_templates {
                    for template in &templates {
                        let effective =
                            class_template.map(|class| join_route_templates(class, template));
                        let source = effective.as_deref().unwrap_or(template);
                        for verb in &verbs {
                            if let Some(fact) = mapping_fact(
                                language,
                                tree,
                                file_path,
                                content,
                                target_start,
                                target_end,
                                mapping.attribute_kind,
                                template,
                                source,
                                class_template,
                                effective.as_deref(),
                                *verb,
                            ) {
                                facts.push(fact);
                            }
                        }
                    }
                }
            }
        }
        if is_java_class_declaration(trimmed) && !mask.is_string_or_comment(line_offset) {
            // Each class declaration owns its own class-level template; a class
            // without a class-level mapping resets it so the previous
            // controller's prefix cannot leak into this one's routes.
            let pending = pending_class_mapping.take();
            current_class_templates = pending
                .as_ref()
                .map(|(mapping, _, _)| mapping.templates.clone())
                .unwrap_or_default();
            if let Some((mapping, start, end)) = pending {
                for template in mapping.templates {
                    if let Some(fact) = mapping_fact(
                        language,
                        tree,
                        file_path,
                        content,
                        start,
                        end,
                        "class_route",
                        &template,
                        &template,
                        None,
                        None,
                        None,
                    ) {
                        facts.push(fact);
                    }
                }
            }
        }
        offset += line.len();
        index += 1;
    }
    facts
}

#[derive(Clone)]
struct MappingAnnotation {
    templates: Vec<String>,
    verbs: Vec<String>,
    attribute_kind: &'static str,
    has_route_argument: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeclarationKind {
    Class,
    Method,
}

fn parse_mapping_annotation(
    content: &str,
    mask: &SourceMask,
    line_start: usize,
    line: &str,
) -> Option<MappingAnnotation> {
    let annotation_start = line.find('@')?;
    let name_start = line_start + annotation_start + 1;
    let after_name = &content[name_start..];
    let name_len = after_name.find(['(', ' ', '\n', '\r'])?;
    let annotation = &after_name[..name_len];
    let default_verb = match annotation {
        "GetMapping" => Some("GET"),
        "PostMapping" => Some("POST"),
        "PutMapping" => Some("PUT"),
        "PatchMapping" => Some("PATCH"),
        "DeleteMapping" => Some("DELETE"),
        "RequestMapping" => None,
        _ => return None,
    };
    let attribute_kind = if default_verb.is_some() {
        "http_method"
    } else {
        "request_mapping"
    };
    let open = skip_ascii_whitespace_until(content, name_start + name_len, content.len());
    if content.as_bytes().get(open) != Some(&b'(') {
        return Some(MappingAnnotation {
            templates: vec!["".to_string()],
            verbs: default_verb.map(str::to_string).into_iter().collect(),
            attribute_kind,
            has_route_argument: false,
        });
    }
    let close = find_matching_paren(content, mask, open)?;
    let args = &content[open + 1..close];
    let elements = parse_annotation_elements(args);
    let templates = if elements.templates.is_empty() && args.trim().is_empty() {
        vec!["".to_string()]
    } else {
        elements.templates
    };
    let verbs = default_verb
        .map(|verb| vec![verb.to_string()])
        .unwrap_or(elements.verbs);
    Some(MappingAnnotation {
        templates,
        verbs,
        attribute_kind,
        has_route_argument: elements.has_route_argument,
    })
}

#[derive(Default)]
struct AnnotationElements {
    templates: Vec<String>,
    verbs: Vec<String>,
    has_route_argument: bool,
}

/// Splits annotation arguments into named elements. Route templates come only
/// from the positional value or the `value =` / `path =` elements; string
/// literals in `produces`/`consumes`/`params`/`headers` are not routes.
fn parse_annotation_elements(args: &str) -> AnnotationElements {
    let mask = SourceMask::new(args, MaskLanguage::Java);
    let mut elements = AnnotationElements::default();
    let mut cursor = 0;
    while cursor < args.len() {
        cursor = skip_ascii_whitespace_until(args, cursor, args.len());
        if cursor >= args.len() {
            break;
        }
        let element_end = find_top_level_comma_or_end(args, &mask, cursor, args.len());
        let element = &args[cursor..element_end];
        let (name, value) = split_annotation_element(element);
        match name {
            None | Some("value") | Some("path") => {
                elements.has_route_argument = true;
                collect_string_values(args, &mask, cursor + value_offset(element, value), value)
                    .into_iter()
                    .for_each(|template| elements.templates.push(template));
            }
            Some("method") => {
                for verb in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
                    if value.contains(&format!("RequestMethod.{verb}")) {
                        elements.verbs.push(verb.to_string());
                    }
                }
            }
            _ => {}
        }
        cursor = element_end.saturating_add(1);
    }
    elements
}

fn split_annotation_element(element: &str) -> (Option<&str>, &str) {
    let trimmed = element.trim_start();
    let Some(equals) = trimmed.find('=') else {
        return (None, element);
    };
    let name = trimmed[..equals].trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return (None, element);
    }
    (Some(name), &trimmed[equals + 1..])
}

fn value_offset(element: &str, value: &str) -> usize {
    element.len() - value.len()
}

/// Collects the string literal(s) of one annotation element value: either a
/// single literal or a `{ "a", "b" }` array initializer.
fn collect_string_values(
    args: &str,
    mask: &SourceMask,
    value_start: usize,
    value: &str,
) -> Vec<String> {
    let trimmed_start = value_start + (value.len() - value.trim_start().len());
    if args.as_bytes().get(trimmed_start) == Some(&b'{') {
        let Some(close) = find_matching_brace_within(args, mask, trimmed_start, args.len()) else {
            return Vec::new();
        };
        let mut values = Vec::new();
        let mut cursor = trimmed_start + 1;
        while cursor < close {
            cursor = skip_ascii_whitespace_until(args, cursor, close);
            if cursor >= close {
                break;
            }
            let Some((literal, literal_end)) = parse_java_string_literal(args, cursor) else {
                break;
            };
            values.push(literal);
            cursor = skip_ascii_whitespace_until(args, literal_end, close);
            if args.as_bytes().get(cursor) == Some(&b',') {
                cursor += 1;
            }
        }
        return values;
    }
    parse_java_string_literal(args, trimmed_start)
        .map(|(literal, _)| vec![literal])
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn mapping_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    start: usize,
    end: usize,
    attribute_kind: &str,
    route_template: &str,
    normalized_source: &str,
    class_route_template: Option<&str>,
    effective_route_template: Option<&str>,
    verb: Option<&str>,
) -> Option<StructuralFact> {
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let normalized = normalize_route_template(normalized_source, ParamFlavor::Braces);
    let mut metadata = base_metadata("framework", "spring");
    insert_string(&mut metadata, "api_style", "annotation_routing");
    insert_string(&mut metadata, "attribute_kind", attribute_kind);
    insert_string(&mut metadata, "route_template", route_template);
    insert_string(
        &mut metadata,
        "normalized_route_template",
        &normalized.template,
    );
    if !normalized.dynamic_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "dynamic_segments",
            normalized.dynamic_segments,
        );
    }
    if let Some(class_route_template) = class_route_template {
        insert_string(&mut metadata, "class_route_template", class_route_template);
    }
    if let Some(effective_route_template) = effective_route_template {
        insert_string(
            &mut metadata,
            "effective_route_template",
            effective_route_template,
        );
    }
    if let Some(verb) = verb {
        insert_string(&mut metadata, "verb", verb);
        insert_string(&mut metadata, "verb_source", "attested");
    }
    Some(fact_for_span(
        file_path,
        language,
        SPRING_REQUEST_MAPPING_PATTERN_ID,
        "request_mapping",
        node.kind(),
        span,
        metadata,
    ))
}

fn next_java_declaration_line(
    content: &str,
    start: usize,
) -> Option<(usize, usize, DeclarationKind)> {
    let mut cursor = start;
    while cursor < content.len() {
        let line_end = content[cursor..]
            .find('\n')
            .map(|offset| cursor + offset)
            .unwrap_or(content.len());
        let line = content[cursor..line_end].trim();
        if line.is_empty() || line.starts_with('@') {
            cursor = line_end.saturating_add(1);
            continue;
        }
        let kind = if is_java_class_declaration(line) {
            DeclarationKind::Class
        } else {
            DeclarationKind::Method
        };
        let start = cursor + content[cursor..line_end].find(line).unwrap_or(0);
        return Some((start, line_end, kind));
    }
    None
}

fn is_java_class_declaration(line: &str) -> bool {
    line.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .any(|token| matches!(token, "class" | "interface" | "enum" | "record"))
}

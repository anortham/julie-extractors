use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Tree;

use super::SPRING_REQUEST_MAPPING_PATTERN_ID;
use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node,
    skip_ascii_whitespace_until, smallest_node_covering_range,
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

    let mut facts = Vec::new();
    let mut pending_class_mapping: Option<(MappingAnnotation, usize, usize)> = None;
    let mut current_class_template: Option<String> = None;
    let mut offset = 0;
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        if trimmed.starts_with('@')
            && let Some(mapping) = parse_mapping_annotation(content, offset, line)
        {
            let (target_start, target_end, target_kind) = next_java_declaration_line(
                content,
                offset + line.len(),
            )
            .unwrap_or((offset, offset + line.len(), DeclarationKind::Method));
            if target_kind == DeclarationKind::Class {
                current_class_template = mapping.templates.first().cloned();
                pending_class_mapping = Some((mapping, target_start, target_end));
            } else {
                let templates = if mapping.templates.is_empty() {
                    vec!["".to_string()]
                } else {
                    mapping.templates.clone()
                };
                for template in templates {
                    let effective = current_class_template
                        .as_deref()
                        .map(|class| join_route_templates(class, &template));
                    let source = effective.as_deref().unwrap_or(&template);
                    let verbs = if mapping.verbs.is_empty() {
                        vec![None]
                    } else {
                        mapping
                            .verbs
                            .iter()
                            .map(|verb| Some(verb.as_str()))
                            .collect()
                    };
                    for verb in verbs {
                        if let Some(fact) = mapping_fact(
                            language,
                            tree,
                            file_path,
                            content,
                            target_start,
                            target_end,
                            "http_method",
                            &template,
                            source,
                            current_class_template.as_deref(),
                            effective.as_deref(),
                            verb,
                        ) {
                            facts.push(fact);
                        }
                    }
                }
            }
        }
        if is_java_class_declaration(trimmed)
            && let Some((mapping, start, end)) = pending_class_mapping.take()
        {
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
        offset += line.len();
        index += 1;
    }
    facts
}

#[derive(Clone)]
struct MappingAnnotation {
    templates: Vec<String>,
    verbs: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeclarationKind {
    Class,
    Method,
}

fn parse_mapping_annotation(
    content: &str,
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
    let open = skip_ascii_whitespace_until(content, name_start + name_len, content.len());
    if content.as_bytes().get(open) != Some(&b'(') {
        return Some(MappingAnnotation {
            templates: vec!["".to_string()],
            verbs: default_verb.map(str::to_string).into_iter().collect(),
        });
    }
    let close = find_matching_paren(content, open)?;
    let args = &content[open + 1..close];
    let mut templates = string_literals(args);
    if args.contains("method") {
        templates.retain(|template| {
            !matches!(
                template.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
            )
        });
    }
    let verbs = default_verb
        .map(|verb| vec![verb.to_string()])
        .unwrap_or_else(|| request_mapping_verbs(args));
    Some(MappingAnnotation { templates, verbs })
}

fn request_mapping_verbs(args: &str) -> Vec<String> {
    let mut verbs = Vec::new();
    for verb in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        if args.contains(&format!("RequestMethod.{verb}")) {
            verbs.push(verb.to_string());
        }
    }
    verbs
}

fn string_literals(input: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor < input.len() {
        if let Some((value, end)) = parse_java_string_literal(input, cursor) {
            values.push(value);
            cursor = end;
        } else {
            cursor += 1;
        }
    }
    if values.is_empty() && input.trim().is_empty() {
        values.push("".to_string());
    }
    values
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
    line.starts_with("class ")
        || line.contains(" class ")
        || line.starts_with("public class ")
        || line.starts_with("private class ")
        || line.starts_with("protected class ")
}

fn insert_string_array(metadata: &mut HashMap<String, Value>, key: &str, values: Vec<String>) {
    metadata.insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
}

fn parse_java_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    if content.as_bytes().get(start) != Some(&b'"') {
        return None;
    }
    let mut cursor = start + 1;
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if byte == b'\\' {
            let escaped_start = cursor + 1;
            let escaped = content.get(escaped_start..)?.chars().next()?;
            value.push(escaped);
            cursor = escaped_start + escaped.len_utf8();
        } else if byte == b'"' {
            return Some((value, cursor + 1));
        } else {
            let ch = content.get(cursor..)?.chars().next()?;
            value.push(ch);
            cursor += ch.len_utf8();
        }
    }
    None
}

fn find_matching_paren(content: &str, open: usize) -> Option<usize> {
    if content.as_bytes().get(open) != Some(&b'(') {
        return None;
    }
    let mut cursor = open;
    let mut depth = 0usize;
    let mut quote = false;
    let mut escaped = false;
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quote = false;
            }
        } else if byte == b'"' {
            quote = true;
        } else if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }
    None
}

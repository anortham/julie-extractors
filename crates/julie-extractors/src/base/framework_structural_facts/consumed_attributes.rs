use tree_sitter::Tree;

use super::HTMX_ATTRIBUTE_PATTERN_ID;
use super::helpers::{
    fact_for_span, is_ignored_markup_node, parse_csharp_string_literal,
    smallest_node_covering_range,
};
use super::markup::{canonical_htmx_attribute_name, htmx_attribute_metadata};
use super::scan::{
    MaskLanguage, SourceMask, find_matching_brace_within, find_top_level_comma_or_end,
};
use crate::base::markup_scan::scan_markup_attributes;
use crate::base::span::NormalizedSpan;
use crate::base::types::{StructuralFact, Symbol, stable_location_id};
use crate::base::web_structural_facts::js_object_scan::{
    parse_js_identifier, parse_js_string_literal,
};

struct Binding<'a> {
    expression: &'a str,
    start: usize,
    site_start: usize,
    site_end: usize,
}

pub(super) fn collect_consumed_attributes(
    language: &str,
    tree: &Tree,
    path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    if !matches!(
        language,
        "razor" | "html" | "vue" | "javascript" | "jsx" | "tsx"
    ) {
        return Vec::new();
    }
    let mut bindings = Vec::new();
    if matches!(language, "javascript" | "jsx" | "tsx") {
        let mut pending = vec![tree.root_node()];
        while let Some(node) = pending.pop() {
            if node.kind() == "jsx_expression"
                && node.parent().is_some_and(|parent| {
                    matches!(
                        parent.kind(),
                        "jsx_opening_element" | "jsx_self_closing_element"
                    )
                })
                && let Some(raw) = content.get(node.byte_range())
                && let Some(expression) = raw
                    .strip_prefix("{...")
                    .and_then(|text| text.strip_suffix('}'))
            {
                bindings.push(Binding {
                    expression,
                    start: node.start_byte() + 4,
                    site_start: node.start_byte(),
                    site_end: node.end_byte(),
                });
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
    } else {
        let ranges = if language == "vue" {
            crate::base::web_structural_facts::vue_template_section_ranges(content)
        } else {
            vec![(0, content.len())]
        };
        for (start, end) in ranges {
            for attribute in scan_markup_attributes(content, start, end) {
                let consumes = matches!(
                    (language, attribute.name.as_str()),
                    ("razor", "@attributes") | ("vue", "v-bind") | ("html", "x-bind")
                );
                if !consumes {
                    continue;
                }
                let Some(node) = smallest_node_covering_range(
                    tree.root_node(),
                    attribute.start_byte,
                    attribute.end_byte,
                ) else {
                    continue;
                };
                if is_ignored_markup_node(node) {
                    continue;
                }
                if let Some(value) = attribute.value.as_deref()
                    && let Some(offset) =
                        content[attribute.start_byte..attribute.end_byte].find(value)
                {
                    let start = attribute.start_byte + offset;
                    bindings.push(Binding {
                        expression: &content[start..start + value.len()],
                        start,
                        site_start: attribute.start_byte,
                        site_end: attribute.end_byte,
                    });
                }
            }
        }
    }
    let mut facts = Vec::new();
    for binding in bindings {
        let expression = binding.expression.trim();
        let is_inline = expression.starts_with('{');
        let declaration = if is_inline {
            Some((
                binding.start + binding.expression.find(expression).unwrap_or(0),
                expression,
            ))
        } else {
            let name = expression.strip_prefix("this.").unwrap_or(expression);
            let mut candidates = symbols.iter().filter(|symbol| symbol.name == name);
            match (candidates.next(), candidates.next()) {
                (Some(symbol), None) => content
                    .get(symbol.start_byte as usize..symbol.end_byte as usize)
                    .map(|text| (symbol.start_byte as usize, text)),
                (None, None) if language == "vue" => vue_declaration(content, name),
                _ => None,
            }
        };
        let Some((base, declaration)) = declaration else {
            continue;
        };
        for (start, end) in object_bodies(declaration, language == "razor", is_inline) {
            let mask = SourceMask::new(
                declaration,
                if language == "razor" {
                    MaskLanguage::CSharp
                } else {
                    MaskLanguage::Js
                },
            );
            let mut cursor = start;
            while cursor < end {
                let next = find_top_level_comma_or_end(declaration, &mask, cursor, end);
                let raw = declaration[cursor..next].trim();
                if let Some((name, value)) = entry(raw, language == "razor")
                    && let Some((name, data_prefix)) = canonical_htmx_attribute_name(&name)
                {
                    let static_value = literal(value, language == "razor");
                    let mut metadata = htmx_attribute_metadata(
                        &name,
                        Some(static_value.as_deref().unwrap_or(value)),
                        data_prefix,
                    );
                    metadata.insert(
                        "value_source".into(),
                        if static_value.is_some() {
                            "string_literal"
                        } else {
                            "dynamic_expression"
                        }
                        .into(),
                    );
                    if static_value.is_none() {
                        metadata.remove("target_path");
                    }
                    metadata.insert("binding_source".into(), "consumed_object".into());
                    super::htmx_templates::enrich_htmx_template(language, value, &mut metadata);
                    metadata.insert("declaration_start_byte".into(), (base + cursor).into());
                    metadata.insert("declaration_end_byte".into(), (base + next).into());
                    if let Some(span) = NormalizedSpan::from_content_range(
                        content,
                        binding.site_start,
                        binding.site_end,
                    ) && let Some(node) = smallest_node_covering_range(
                        tree.root_node(),
                        binding.site_start,
                        binding.site_end,
                    ) {
                        let mut fact = fact_for_span(
                            path,
                            language,
                            HTMX_ATTRIBUTE_PATTERN_ID,
                            "attribute",
                            node.kind(),
                            span,
                            metadata,
                        );
                        fact.id = stable_location_id(
                            path,
                            &format!("{HTMX_ATTRIBUTE_PATTERN_ID}:attribute:{name}"),
                            span,
                        );
                        facts.push(fact);
                    }
                }
                cursor = next + 1;
            }
        }
    }
    facts.sort_by(|a, b| a.id.cmp(&b.id));
    facts.dedup_by(|a, b| a.id == b.id);
    facts
}

fn object_bodies(text: &str, csharp: bool, inline: bool) -> Vec<(usize, usize)> {
    let mask = SourceMask::new(
        text,
        if csharp {
            MaskLanguage::CSharp
        } else {
            MaskLanguage::Js
        },
    );
    if !csharp {
        let start = if inline {
            0
        } else {
            let Some(equals) = text
                .bytes()
                .enumerate()
                .find(|(i, b)| *b == b'=' && !mask.is_string_or_comment(*i))
                .map(|(i, _)| i)
            else {
                return Vec::new();
            };
            equals + 1
        };
        let expression = text[start..].trim_start();
        let start = start + text[start..].len() - expression.len();
        if !expression.starts_with('{') {
            return Vec::new();
        }
        return find_matching_brace_within(text, &mask, start, text.len())
            .map(|end| vec![(start + 1, end)])
            .unwrap_or_default();
    }
    let mut ranges = Vec::new();
    for (index, _) in text.match_indices("new") {
        if mask.is_string_or_comment(index) {
            continue;
        }
        let prefix = text[..index].trim_end();
        if !prefix.ends_with('=')
            && !prefix.ends_with("=>")
            && !prefix.ends_with('?')
            && !prefix.ends_with(':')
        {
            continue;
        }
        let Some(open) = text[index..].find('{').map(|offset| index + offset) else {
            continue;
        };
        let construction = text[index..open].trim();
        if !construction.contains("Dictionary")
            && !(construction == "new()" && text[..index].contains("Dictionary"))
        {
            continue;
        }
        if let Some(end) = find_matching_brace_within(text, &mask, open, text.len()) {
            ranges.push((open + 1, end));
        }
    }
    ranges
}

fn entry(text: &str, csharp: bool) -> Option<(String, &str)> {
    if csharp {
        if let Some(rest) = text.strip_prefix('[') {
            let end = rest.find(']')?;
            let key = literal(rest[..end].trim(), true)?;
            let value = rest[end + 1..].trim_start().strip_prefix('=')?.trim();
            return Some((key, value));
        }
        let pair = text.strip_prefix('{')?.strip_suffix('}')?;
        let mask = SourceMask::new(pair, MaskLanguage::CSharp);
        let comma = find_top_level_comma_or_end(pair, &mask, 0, pair.len());
        return Some((
            literal(pair[..comma].trim(), true)?,
            pair.get(comma + 1..)?.trim(),
        ));
    }
    let mask = SourceMask::new(text, MaskLanguage::Js);
    let colon = text
        .bytes()
        .enumerate()
        .find(|(i, b)| *b == b':' && !mask.is_string_or_comment(*i))?
        .0;
    Some((
        literal(text[..colon].trim(), false)?,
        text[colon + 1..].trim(),
    ))
}

fn literal(text: &str, csharp: bool) -> Option<String> {
    if csharp {
        return parse_csharp_string_literal(text, 0)
            .filter(|(_, end, _)| *end == text.len())
            .map(|(value, _, _)| value);
    }
    parse_js_string_literal(text, 0)
        .filter(|(_, end)| *end == text.len())
        .map(|(value, _)| value)
}

fn vue_declaration<'a>(content: &'a str, name: &str) -> Option<(usize, &'a str)> {
    let mut matches = Vec::new();
    for (section_start, section_end) in
        crate::base::web_structural_facts::vue_script_section_ranges(content)
    {
        let script = &content[section_start..section_end];
        let mask = SourceMask::new(script, MaskLanguage::Js);
        for keyword in ["const", "let", "var"] {
            for (index, _) in script.match_indices(keyword) {
                if mask.is_string_or_comment(index)
                    || index
                        .checked_sub(1)
                        .is_some_and(|i| script.as_bytes()[i].is_ascii_alphanumeric())
                {
                    continue;
                }
                let tail = script.get(index + keyword.len()..)?;
                if !tail.starts_with(char::is_whitespace) {
                    continue;
                }
                let start = script.len() - tail.trim_start().len();
                let Some((identifier, end)) = parse_js_identifier(script, start, script.len())
                else {
                    continue;
                };
                if identifier != name {
                    continue;
                }
                let tail = script[end..].trim_start();
                if !tail.starts_with('=') && !tail.starts_with(':') {
                    continue;
                }
                let statement_end = super::scan::statement_end(script, &mask, index, true);
                matches.push((section_start + index, &script[index..statement_end]));
            }
        }
    }
    if matches.len() == 1 {
        matches.pop()
    } else {
        None
    }
}

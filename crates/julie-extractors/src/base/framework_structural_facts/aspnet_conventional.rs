use tree_sitter::Tree;

use super::ASPNET_CONVENTIONAL_ROUTE_PATTERN_ID;
use super::helpers::{
    base_metadata, fact_for_span, find_matching_paren, find_top_level_comma_or_end, insert_string,
    is_comment_or_string_node, is_identifier_boundary, parse_csharp_string_literal,
    skip_ascii_whitespace, skip_ascii_whitespace_until, smallest_node_covering_range,
};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

const DEFAULT_ROUTE_TEMPLATE: &str = "{controller=Home}/{action=Index}/{id?}";

/// Positional parameter order of each conventional route map.
const CONVENTIONAL_ROUTE_METHODS: &[(&str, &[&str])] = &[
    ("MapControllerRoute", &["name", "pattern"]),
    ("MapAreaControllerRoute", &["name", "areaName", "pattern"]),
    ("MapDefaultControllerRoute", &[]),
];

/// `aspnet.conventional_route.v1` facts for `MapControllerRoute`,
/// `MapAreaControllerRoute`, and `MapDefaultControllerRoute` calls. Named
/// arguments (`name: "x"`, VB `name:="x"`) and positional arguments both
/// count; only string-literal values are read.
pub(super) fn collect_aspnet_conventional_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for (method_name, parameters) in CONVENTIONAL_ROUTE_METHODS {
        let mut search_start = 0;
        while let Some(relative_start) = content[search_start..].find(method_name) {
            let method_start = search_start + relative_start;
            search_start = method_start + method_name.len();
            if !is_identifier_boundary(content, method_start, method_name.len()) {
                continue;
            }
            let open_paren = skip_ascii_whitespace(content, search_start);
            if content.as_bytes().get(open_paren) != Some(&b'(') {
                continue;
            }
            let Some(close_paren) = find_matching_paren(content, open_paren) else {
                continue;
            };
            let Some(node) =
                smallest_node_covering_range(tree.root_node(), method_start, close_paren + 1)
            else {
                continue;
            };
            if is_comment_or_string_node(node.kind()) {
                continue;
            }
            let Some(span) =
                NormalizedSpan::from_content_range(content, method_start, close_paren + 1)
            else {
                continue;
            };

            let arguments = string_arguments(content, open_paren + 1, close_paren, parameters);
            let value = |parameter: &str| {
                arguments
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(parameter))
                    .map(|(_, value)| value.as_str())
            };
            let (route_name, route_template) = if parameters.is_empty() {
                (Some("default"), Some(DEFAULT_ROUTE_TEMPLATE))
            } else {
                (value("name"), value("pattern"))
            };
            let Some(route_template) = route_template else {
                continue;
            };

            let mut metadata = base_metadata("framework", "aspnet");
            insert_string(&mut metadata, "api_style", "conventional_routing");
            insert_string(&mut metadata, "route_template", route_template);
            if let Some(route_name) = route_name {
                insert_string(&mut metadata, "route_name", route_name);
            }
            if let Some(area_name) = value("areaName") {
                insert_string(&mut metadata, "area_name", area_name);
            }
            facts.push(fact_for_span(
                file_path,
                language,
                ASPNET_CONVENTIONAL_ROUTE_PATTERN_ID,
                "conventional_route",
                node.kind(),
                span,
                metadata,
            ));
        }
    }
    facts
}

/// `(parameter, value)` for every string-literal argument, naming positional
/// arguments by `parameters` order.
fn string_arguments(
    content: &str,
    start: usize,
    end: usize,
    parameters: &[&str],
) -> Vec<(String, String)> {
    let mut arguments = Vec::new();
    let mut cursor = start;
    let mut position = 0;
    while cursor < end {
        let argument_end = find_top_level_comma_or_end(content, cursor, end);
        let argument = content[cursor..argument_end].trim();
        let (label, value_text) = match argument.split_once(':') {
            Some((label, value))
                if !label.trim().is_empty()
                    && label
                        .trim()
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_') =>
            {
                (
                    Some(label.trim().to_string()),
                    value.trim_start_matches('=').trim(),
                )
            }
            _ => (None, argument),
        };
        let name = label.or_else(|| parameters.get(position).map(|p| p.to_string()));
        if let (Some(name), Some((value, value_end, _))) =
            (name, parse_csharp_string_literal(value_text, 0))
            && skip_ascii_whitespace_until(value_text, value_end, value_text.len())
                == value_text.len()
        {
            arguments.push((name, value));
        }
        position += 1;
        cursor = argument_end + 1;
    }
    arguments
}

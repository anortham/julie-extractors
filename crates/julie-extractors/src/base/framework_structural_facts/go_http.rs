use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node,
    skip_ascii_whitespace_until, smallest_node_covering_range,
};
use super::{ECHO_ROUTE_PATTERN_ID, GIN_ROUTE_PATTERN_ID, GO_NET_HTTP_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

pub(super) fn collect_go_http_boundary_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports = collect_go_imports(content);
    let mut facts = Vec::new();
    if let Some(http_alias) = imports.net_http.as_deref() {
        let muxes = collect_muxes(content, http_alias);
        facts.extend(collect_net_http_routes(
            language, tree, file_path, content, http_alias, &muxes,
        ));
    }
    if let Some(gin_alias) = imports.gin.as_deref() {
        let (routers, groups) = collect_grouped_receivers(content, gin_alias, &["Default", "New"]);
        facts.extend(collect_group_framework_routes(
            language,
            tree,
            file_path,
            content,
            "gin",
            GIN_ROUTE_PATTERN_ID,
            ParamFlavor::GinWildcard,
            &routers,
            &groups,
        ));
    }
    if let Some(echo_alias) = imports.echo.as_deref() {
        let (routers, groups) = collect_grouped_receivers(content, echo_alias, &["New"]);
        facts.extend(collect_group_framework_routes(
            language,
            tree,
            file_path,
            content,
            "echo",
            ECHO_ROUTE_PATTERN_ID,
            ParamFlavor::Colon,
            &routers,
            &groups,
        ));
    }
    facts
}

#[derive(Default)]
struct GoImports {
    net_http: Option<String>,
    gin: Option<String>,
    echo: Option<String>,
}

fn collect_go_imports(content: &str) -> GoImports {
    let mut imports = GoImports::default();
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches("import ").trim();
        for (path, slot, default_alias) in [
            ("net/http", 0usize, "http"),
            ("github.com/gin-gonic/gin", 1, "gin"),
            ("github.com/labstack/echo/v4", 2, "echo"),
        ] {
            if trimmed == format!("\"{path}\"") {
                set_alias(&mut imports, slot, default_alias.to_string());
            } else if trimmed.ends_with(&format!("\"{path}\"")) {
                let alias = trimmed.trim_end_matches(&format!("\"{path}\"")).trim();
                if !alias.is_empty() && alias != "_" && alias != "." {
                    set_alias(&mut imports, slot, alias.to_string());
                }
            }
        }
    }
    imports
}

fn set_alias(imports: &mut GoImports, slot: usize, alias: String) {
    match slot {
        0 => imports.net_http = Some(alias),
        1 => imports.gin = Some(alias),
        2 => imports.echo = Some(alias),
        _ => {}
    }
}

fn collect_muxes(content: &str, http_alias: &str) -> HashSet<String> {
    let mut muxes = HashSet::new();
    collect_assignment_names(content, &format!("{http_alias}.NewServeMux()"), &mut muxes);
    muxes
}

fn collect_grouped_receivers(
    content: &str,
    import_alias: &str,
    constructors: &[&str],
) -> (HashSet<String>, HashMap<String, String>) {
    let mut routers = HashSet::new();
    for constructor in constructors {
        collect_assignment_names(
            content,
            &format!("{import_alias}.{constructor}()"),
            &mut routers,
        );
    }
    let mut groups = HashMap::new();
    for router in routers.clone() {
        let needle = format!("{router}.Group");
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find(&needle) {
            let call_start = cursor + relative;
            cursor = call_start + needle.len();
            let open = skip_ascii_whitespace_until(content, cursor, content.len());
            if content.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let Some(close) = find_matching_paren(content, open) else {
                continue;
            };
            let arg_start = skip_ascii_whitespace_until(content, open + 1, close);
            let Some((prefix, _)) = parse_go_string_literal(content, arg_start) else {
                continue;
            };
            let statement_start = content[..call_start]
                .rfind(['\n', ';'])
                .map(|index| index + 1)
                .unwrap_or(0);
            let before = content[statement_start..call_start].trim();
            if let Some(name) = before
                .split(":=")
                .next()
                .map(str::trim)
                .filter(|name| is_go_identifier(name))
            {
                groups.insert(name.to_string(), prefix);
            }
        }
    }
    (routers, groups)
}

fn collect_assignment_names(content: &str, needle: &str, names: &mut HashSet<String>) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        let statement_start = content[..call_start]
            .rfind(['\n', ';'])
            .map(|index| index + 1)
            .unwrap_or(0);
        let before = content[statement_start..call_start].trim();
        if let Some(name) = before
            .split(":=")
            .next()
            .map(str::trim)
            .filter(|name| is_go_identifier(name))
        {
            names.insert(name.to_string());
        }
    }
}

fn collect_net_http_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    http_alias: &str,
    muxes: &HashSet<String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut receivers = vec![http_alias.to_string()];
    receivers.extend(muxes.iter().cloned());
    for receiver in receivers {
        for method in ["Handle", "HandleFunc"] {
            collect_route_calls(
                language,
                tree,
                file_path,
                content,
                &format!("{receiver}.{method}"),
                "net/http",
                GO_NET_HTTP_ROUTE_PATTERN_ID,
                ParamFlavor::BracesWithDots,
                None,
                &mut facts,
            );
        }
    }
    facts
}

#[allow(clippy::too_many_arguments)]
fn collect_group_framework_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    framework: &str,
    pattern_id: &str,
    flavor: ParamFlavor,
    routers: &HashSet<String>,
    groups: &HashMap<String, String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut receivers = routers
        .iter()
        .map(|name| (name.as_str(), None))
        .collect::<Vec<_>>();
    receivers.extend(
        groups
            .iter()
            .map(|(name, prefix)| (name.as_str(), Some(prefix.as_str()))),
    );
    for (receiver, prefix) in receivers {
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
            collect_route_calls(
                language,
                tree,
                file_path,
                content,
                &format!("{receiver}.{method}"),
                framework,
                pattern_id,
                flavor,
                prefix,
                &mut facts,
            );
        }
    }
    facts
}

#[allow(clippy::too_many_arguments)]
fn collect_route_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    needle: &str,
    framework: &str,
    pattern_id: &str,
    flavor: ParamFlavor,
    prefix: Option<&str>,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if is_in_go_string_or_comment(content, call_start) {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, open) else {
            continue;
        };
        let arg_start = skip_ascii_whitespace_until(content, open + 1, close);
        let arg_end = find_top_level_comma_or_end(content, arg_start, close);
        let Some((raw_pattern, pattern_end)) = parse_go_string_literal(content, arg_start) else {
            continue;
        };
        if skip_ascii_whitespace_until(content, pattern_end, arg_end) != arg_end {
            continue;
        }
        let (verb, route_template) = if framework == "net/http" {
            split_go_pattern(&raw_pattern)
        } else {
            (needle.rsplit('.').next().map(str::to_string), raw_pattern)
        };
        if let Some(fact) = route_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            framework,
            pattern_id,
            &route_template,
            verb.as_deref(),
            flavor,
            prefix,
        ) {
            facts.push(fact);
        }
    }
}

fn split_go_pattern(pattern: &str) -> (Option<String>, String) {
    let mut parts = pattern.split_whitespace();
    let first = parts.next().unwrap_or(pattern);
    let second = parts.next();
    if matches!(
        first,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    ) && let Some(path) = second
    {
        return (Some(first.to_string()), path.to_string());
    }
    (None, pattern.to_string())
}

#[allow(clippy::too_many_arguments)]
fn route_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    start: usize,
    end: usize,
    framework: &str,
    pattern_id: &str,
    route_template: &str,
    verb: Option<&str>,
    flavor: ParamFlavor,
    prefix: Option<&str>,
) -> Option<StructuralFact> {
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let mut metadata = base_metadata("framework", framework);
    insert_string(&mut metadata, "api_style", "mux_routing");
    insert_string(&mut metadata, "route_template", route_template);
    let normalized_source = if let Some(prefix) = prefix {
        insert_string(&mut metadata, "route_group_prefix", prefix);
        let effective = join_route_templates(prefix, route_template);
        insert_string(&mut metadata, "effective_route_template", &effective);
        effective
    } else {
        route_template.to_string()
    };
    let normalized = normalize_route_template(&normalized_source, flavor);
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
    if let Some(verb) = verb {
        insert_string(&mut metadata, "verb", verb);
        insert_string(&mut metadata, "verb_source", "attested");
    }
    Some(fact_for_span(
        file_path,
        language,
        pattern_id,
        "route_call",
        node.kind(),
        span,
        metadata,
    ))
}

fn insert_string_array(metadata: &mut HashMap<String, Value>, key: &str, values: Vec<String>) {
    metadata.insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
}

fn parse_go_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let quote = content.as_bytes().get(start).copied()?;
    if quote == b'`' {
        let end = content[start + 1..].find('`')? + start + 1;
        return Some((content[start + 1..end].to_string(), end + 1));
    }
    if quote != b'"' {
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
    let mut quote = None;
    let mut escaped = false;
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if let Some(active_quote) = quote {
            if active_quote == b'`' {
                if byte == b'`' {
                    quote = None;
                }
            } else if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if matches!(byte, b'"' | b'`') {
            quote = Some(byte);
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

fn find_top_level_comma_or_end(content: &str, start: usize, end: usize) -> usize {
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while cursor < end {
        let byte = content.as_bytes()[cursor];
        if let Some(active_quote) = quote {
            if active_quote == b'`' {
                if byte == b'`' {
                    quote = None;
                }
            } else if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if matches!(byte, b'"' | b'`') {
            quote = Some(byte);
        } else if byte == b'(' {
            paren_depth += 1;
        } else if byte == b')' {
            paren_depth = paren_depth.saturating_sub(1);
        } else if byte == b',' && paren_depth == 0 {
            return cursor;
        }
        cursor += 1;
    }
    end
}

fn is_go_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_in_go_string_or_comment(content: &str, target: usize) -> bool {
    let bytes = content.as_bytes();
    let mut cursor = 0;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut quote = None;
    let mut escaped = false;
    while cursor < target {
        let byte = bytes[cursor];
        let next = bytes.get(cursor + 1).copied();
        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
        } else if block_comment {
            if byte == b'*' && next == Some(b'/') {
                block_comment = false;
                cursor += 1;
            }
        } else if let Some(active_quote) = quote {
            if active_quote == b'`' {
                if byte == b'`' {
                    quote = None;
                }
            } else if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'/' && next == Some(b'/') {
            line_comment = true;
            cursor += 1;
        } else if byte == b'/' && next == Some(b'*') {
            block_comment = true;
            cursor += 1;
        } else if matches!(byte, b'"' | b'`') {
            quote = Some(byte);
        }
        cursor += 1;
    }
    line_comment || block_comment || quote.is_some()
}

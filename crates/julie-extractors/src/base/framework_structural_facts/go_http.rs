use std::collections::{HashMap, HashSet};

use tree_sitter::Tree;

use super::helpers::{
    insert_string, is_ascii_identifier, is_identifier_boundary, skip_ascii_whitespace_until,
};
use super::scan::{
    MaskLanguage, RouteFactSpec, SourceMask, find_matching_paren, find_top_level_comma_or_end,
    parse_go_string_literal, route_fact,
};
use super::{ECHO_ROUTE_PATTERN_ID, GIN_ROUTE_PATTERN_ID, GO_NET_HTTP_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, join_route_templates};
use crate::base::types::StructuralFact;

const GO_VERBS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

pub(super) fn collect_go_http_boundary_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports = collect_go_imports(content);
    if imports.net_http.is_none() && imports.gin.is_none() && imports.echo.is_none() {
        return Vec::new();
    }
    let mask = SourceMask::new(content, MaskLanguage::Go);
    let mut facts = Vec::new();
    if let Some(http_alias) = imports.net_http.as_deref() {
        let muxes = collect_muxes(content, &mask, http_alias);
        facts.extend(collect_net_http_routes(
            language, tree, file_path, content, &mask, http_alias, &muxes,
        ));
    }
    if let Some(gin_alias) = imports.gin.as_deref() {
        let receivers = collect_grouped_receivers(content, &mask, gin_alias, &["Default", "New"]);
        facts.extend(collect_group_framework_routes(
            language,
            tree,
            file_path,
            content,
            &mask,
            "gin",
            GIN_ROUTE_PATTERN_ID,
            ParamFlavor::GinWildcard,
            &receivers,
        ));
    }
    if let Some(echo_alias) = imports.echo.as_deref() {
        let receivers = collect_grouped_receivers(content, &mask, echo_alias, &["New"]);
        facts.extend(collect_group_framework_routes(
            language,
            tree,
            file_path,
            content,
            &mask,
            "echo",
            ECHO_ROUTE_PATTERN_ID,
            ParamFlavor::Colon,
            &receivers,
        ));
    }
    facts
}

#[derive(Default)]
pub(super) struct GoImports {
    pub(super) net_http: Option<String>,
    pub(super) gin: Option<String>,
    pub(super) echo: Option<String>,
}

pub(super) fn collect_go_imports(content: &str) -> GoImports {
    let mut imports = GoImports::default();
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches("import ").trim();
        let Some((alias, path)) = parse_import_line(trimmed) else {
            continue;
        };
        match path {
            "net/http" => imports.net_http = Some(alias.unwrap_or("http").to_string()),
            "github.com/gin-gonic/gin" => {
                imports.gin = Some(alias.unwrap_or("gin").to_string());
            }
            _ if is_echo_import_path(path) => {
                imports.echo = Some(alias.unwrap_or("echo").to_string());
            }
            _ => {}
        }
    }
    imports
}

/// Splits one import line into an optional alias and the quoted path.
fn parse_import_line(line: &str) -> Option<(Option<&str>, &str)> {
    let open = line.find('"')?;
    let close = line[open + 1..].find('"')? + open + 1;
    let path = &line[open + 1..close];
    let alias = line[..open].trim();
    if alias.is_empty() {
        return Some((None, path));
    }
    if alias == "_" || alias == "." || !is_ascii_identifier(alias) {
        return None;
    }
    Some((Some(alias), path))
}

/// Any major version of labstack/echo counts: `github.com/labstack/echo`,
/// `.../echo/v4`, `.../echo/v5`, ...
fn is_echo_import_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("github.com/labstack/echo") else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    rest.strip_prefix("/v")
        .is_some_and(|version| !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit()))
}

/// A traceable receiver and its composed group prefix. `Poisoned` marks a
/// group created with a non-literal prefix (or derived from one): its routes
/// emit with `route_template` only, never a guessed prefix.
#[derive(Clone, PartialEq, Eq)]
enum GroupPrefix {
    None,
    Literal(String),
    Poisoned,
}

fn collect_muxes(content: &str, mask: &SourceMask, http_alias: &str) -> HashSet<String> {
    let mut muxes = HashSet::new();
    collect_assignment_names(
        content,
        mask,
        &format!("{http_alias}.NewServeMux()"),
        &mut muxes,
    );
    muxes
}

/// Traces routers created from the framework constructors plus every `Group`
/// derived from them (nested groups compose their literal prefixes; a
/// non-literal prefix poisons the chain).
fn collect_grouped_receivers(
    content: &str,
    mask: &SourceMask,
    import_alias: &str,
    constructors: &[&str],
) -> HashMap<String, GroupPrefix> {
    let mut receivers: HashMap<String, GroupPrefix> = HashMap::new();
    let mut roots = HashSet::new();
    for constructor in constructors {
        collect_assignment_names(
            content,
            mask,
            &format!("{import_alias}.{constructor}()"),
            &mut roots,
        );
    }
    for root in roots {
        receivers.insert(root, GroupPrefix::None);
    }

    // Fixpoint over `X.Group(...)` assignments so nested groups resolve
    // regardless of declaration order within the file.
    loop {
        let mut changed = false;
        for (parent, parent_prefix) in receivers.clone() {
            let needle = format!("{parent}.Group");
            let mut cursor = 0;
            while let Some(relative) = content[cursor..].find(&needle) {
                let call_start = cursor + relative;
                cursor = call_start + needle.len();
                if !is_identifier_boundary(content, call_start, needle.len())
                    || mask.is_string_or_comment(call_start)
                {
                    continue;
                }
                let open = skip_ascii_whitespace_until(content, cursor, content.len());
                if content.as_bytes().get(open) != Some(&b'(') {
                    continue;
                }
                let Some(close) = find_matching_paren(content, mask, open) else {
                    continue;
                };
                let arg_start = skip_ascii_whitespace_until(content, open + 1, close);
                let arg_end = find_top_level_comma_or_end(content, mask, arg_start, close);
                let literal =
                    parse_go_string_literal(content, arg_start).filter(|(_, literal_end)| {
                        skip_ascii_whitespace_until(content, *literal_end, arg_end) == arg_end
                    });
                let child_prefix = match (&parent_prefix, literal) {
                    (GroupPrefix::Poisoned, _) | (_, None) => GroupPrefix::Poisoned,
                    (GroupPrefix::None, Some((prefix, _))) => GroupPrefix::Literal(prefix),
                    (GroupPrefix::Literal(parent), Some((prefix, _))) => {
                        GroupPrefix::Literal(join_route_templates(parent, &prefix))
                    }
                };
                let statement_start = content[..call_start]
                    .rfind(['\n', ';'])
                    .map(|index| index + 1)
                    .unwrap_or(0);
                let before = content[statement_start..call_start].trim();
                let Some(name) = before
                    .split(":=")
                    .next()
                    .map(str::trim)
                    .filter(|name| is_ascii_identifier(name))
                else {
                    continue;
                };
                if receivers.get(name) != Some(&child_prefix) {
                    receivers.insert(name.to_string(), child_prefix);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    receivers
}

fn collect_assignment_names(
    content: &str,
    mask: &SourceMask,
    needle: &str,
    names: &mut HashSet<String>,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, needle.len())
            || mask.is_string_or_comment(call_start)
        {
            continue;
        }
        let statement_start = content[..call_start]
            .rfind(['\n', ';'])
            .map(|index| index + 1)
            .unwrap_or(0);
        let before = content[statement_start..call_start].trim();
        if let Some(name) = before
            .split(":=")
            .next()
            .map(str::trim)
            .filter(|name| is_ascii_identifier(name))
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
    mask: &SourceMask,
    http_alias: &str,
    muxes: &HashSet<String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut receivers = vec![http_alias.to_string()];
    receivers.extend(muxes.iter().cloned());
    for receiver in receivers {
        for method in ["Handle", "HandleFunc"] {
            let needle = format!("{receiver}.{method}");
            for call in route_call_sites(content, mask, &needle) {
                let (verb, host, route_template) = split_go_pattern(&call.first_literal);
                if let Some(fact) = route_fact(
                    language,
                    tree,
                    file_path,
                    content,
                    call.start,
                    call.end,
                    RouteFactSpec {
                        framework: "net/http",
                        pattern_id: GO_NET_HTTP_ROUTE_PATTERN_ID,
                        capture_name: "route_call",
                        api_style: "mux_routing",
                        route_template: &route_template,
                        verb: verb.as_deref(),
                        verb_source: verb.is_some().then_some("attested"),
                        flavor: ParamFlavor::BracesWithDots,
                        prefix: None,
                        prefix_key: Some("route_group_prefix"),
                    },
                    |metadata| {
                        if let Some(host) = host.as_deref() {
                            insert_string(metadata, "host", host);
                        }
                    },
                ) {
                    facts.push(fact);
                }
            }
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
    mask: &SourceMask,
    framework: &str,
    pattern_id: &str,
    flavor: ParamFlavor,
    receivers: &HashMap<String, GroupPrefix>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for (receiver, group_prefix) in receivers {
        let prefix = match group_prefix {
            GroupPrefix::Literal(prefix) => Some(prefix.as_str()),
            GroupPrefix::None | GroupPrefix::Poisoned => None,
        };
        let mut emit = |route_template: &str, verb: Option<&str>, start: usize, end: usize| {
            if let Some(fact) = route_fact(
                language,
                tree,
                file_path,
                content,
                start,
                end,
                RouteFactSpec {
                    framework,
                    pattern_id,
                    capture_name: "route_call",
                    api_style: "call_routing",
                    route_template,
                    verb,
                    verb_source: verb.is_some().then_some("attested"),
                    flavor,
                    prefix,
                    prefix_key: Some("route_group_prefix"),
                },
                |_| {},
            ) {
                facts.push(fact);
            }
        };
        for method in GO_VERBS {
            let needle = format!("{receiver}.{method}");
            for call in route_call_sites(content, mask, &needle) {
                emit(&call.first_literal, Some(method), call.start, call.end);
            }
        }
        // `Any(...)` registrations accept every method: verb omitted.
        let needle = format!("{receiver}.Any");
        for call in route_call_sites(content, mask, &needle) {
            emit(&call.first_literal, None, call.start, call.end);
        }
        // gin's `Handle("VERB", "lit", ...)` names the verb as a literal.
        if framework == "gin" {
            let needle = format!("{receiver}.Handle");
            for call in verb_route_call_sites(content, mask, &needle) {
                emit(&call.route_literal, Some(&call.verb), call.start, call.end);
            }
        }
    }
    facts
}

struct RouteCallSite {
    start: usize,
    end: usize,
    first_literal: String,
}

/// Finds `needle("lit", ...)` call sites whose first argument is a single
/// static string literal.
fn route_call_sites(content: &str, mask: &SourceMask, needle: &str) -> Vec<RouteCallSite> {
    let mut sites = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, needle.len())
            || mask.is_string_or_comment(call_start)
        {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, mask, open) else {
            continue;
        };
        let arg_start = skip_ascii_whitespace_until(content, open + 1, close);
        let arg_end = find_top_level_comma_or_end(content, mask, arg_start, close);
        let Some((literal, literal_end)) = parse_go_string_literal(content, arg_start) else {
            continue;
        };
        if skip_ascii_whitespace_until(content, literal_end, arg_end) != arg_end {
            continue;
        }
        sites.push(RouteCallSite {
            start: call_start,
            end: close + 1,
            first_literal: literal,
        });
    }
    sites
}

struct VerbRouteCallSite {
    start: usize,
    end: usize,
    verb: String,
    route_literal: String,
}

/// Finds `needle("VERB", "lit", ...)` call sites where both leading arguments
/// are static string literals.
fn verb_route_call_sites(content: &str, mask: &SourceMask, needle: &str) -> Vec<VerbRouteCallSite> {
    let mut sites = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, needle.len())
            || mask.is_string_or_comment(call_start)
        {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, mask, open) else {
            continue;
        };
        let verb_start = skip_ascii_whitespace_until(content, open + 1, close);
        let verb_end = find_top_level_comma_or_end(content, mask, verb_start, close);
        let Some((verb, verb_literal_end)) = parse_go_string_literal(content, verb_start) else {
            continue;
        };
        if skip_ascii_whitespace_until(content, verb_literal_end, verb_end) != verb_end {
            continue;
        }
        let route_start = skip_ascii_whitespace_until(content, verb_end + 1, close);
        let route_end = find_top_level_comma_or_end(content, mask, route_start, close);
        let Some((route_literal, route_literal_end)) =
            parse_go_string_literal(content, route_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, route_literal_end, route_end) != route_end {
            continue;
        }
        sites.push(VerbRouteCallSite {
            start: call_start,
            end: close + 1,
            verb: verb.to_uppercase(),
            route_literal,
        });
    }
    sites
}

/// Splits a Go 1.22+ ServeMux pattern `[METHOD ][HOST]/[PATH]` into its verb,
/// host, and path parts. `route_template` carries the path part; the host (if
/// any) is recorded separately so host-scoped and host-less routes share a
/// join key.
fn split_go_pattern(pattern: &str) -> (Option<String>, Option<String>, String) {
    let (verb, rest) = match pattern.split_once(' ') {
        Some((first, rest)) if GO_VERBS.contains(&first) => {
            (Some(first.to_string()), rest.trim_start())
        }
        _ => (None, pattern),
    };
    if rest.starts_with('/') {
        return (verb, None, rest.to_string());
    }
    match rest.find('/') {
        Some(slash) if slash > 0 => (
            verb,
            Some(rest[..slash].to_string()),
            rest[slash..].to_string(),
        ),
        _ => (verb, None, rest.to_string()),
    }
}

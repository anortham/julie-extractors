use std::collections::{BTreeSet, HashMap};

use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node, is_identifier_boundary,
    skip_ascii_whitespace_until, smallest_node_covering_range,
};
use super::scan::{
    MaskLanguage, RouteFactSpec, SourceMask, find_matching_brace_within,
    find_matching_bracket_within, find_matching_paren, find_matching_paren_within,
    find_top_level_comma_or_end, route_fact,
};
use super::{EXPRESS_ROUTE_PATTERN_ID, EXPRESS_ROUTER_MOUNT_PATTERN_ID, FASTIFY_ROUTE_PATTERN_ID};
use crate::base::http_boundary::ParamFlavor;
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::base::web_structural_facts::js_imports::{
    js_import_statement_end, parse_default_import, parse_import_source, parse_namespace_import,
};
use crate::base::web_structural_facts::js_object_scan::{
    is_js_identifier, parse_js_identifier, parse_js_string_literal,
};

/// Express and Fastify share the same verb-method surface.
const JS_VERB_METHODS: &[(&str, Option<&str>)] = &[
    ("get", Some("GET")),
    ("post", Some("POST")),
    ("put", Some("PUT")),
    ("patch", Some("PATCH")),
    ("delete", Some("DELETE")),
    ("head", Some("HEAD")),
    ("options", Some("OPTIONS")),
    ("all", None),
];

pub(super) fn collect_node_http_boundary_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mask = SourceMask::new(content, MaskLanguage::Js);
    let imports = collect_node_imports(content, &mask);
    let express_receivers = collect_express_receivers(content, &mask, &imports);
    let fastify_receivers = collect_fastify_receivers(content, &mask, &imports);
    let express_mounts = collect_express_mounts(content, &mask, &express_receivers);

    let mut facts = Vec::new();
    facts.extend(collect_express_route_calls(
        language,
        tree,
        file_path,
        content,
        &mask,
        &express_receivers,
        &express_mounts.same_file_prefixes,
    ));
    facts.extend(collect_express_route_chains(
        language,
        tree,
        file_path,
        content,
        &mask,
        &express_receivers,
        &express_mounts.same_file_prefixes,
    ));
    facts.extend(
        express_mounts
            .facts
            .into_iter()
            .filter_map(|mount| mount.into_fact(language, tree, file_path, content)),
    );
    facts.extend(collect_fastify_route_calls(
        language,
        tree,
        file_path,
        content,
        &mask,
        &fastify_receivers,
    ));
    facts.extend(collect_fastify_route_objects(
        language,
        tree,
        file_path,
        content,
        &mask,
        &fastify_receivers,
    ));
    facts
}

#[derive(Default)]
struct NodeImports {
    express: BTreeSet<String>,
    fastify: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceiverKind {
    ExpressApp,
    ExpressRouter,
}

struct ExpressMounts {
    facts: Vec<MountCandidate>,
    same_file_prefixes: HashMap<String, String>,
}

struct MountCandidate {
    start: usize,
    end: usize,
    mount_path: String,
    mount_target: String,
}

impl MountCandidate {
    fn into_fact(
        self,
        language: &str,
        tree: &Tree,
        file_path: &str,
        content: &str,
    ) -> Option<StructuralFact> {
        let node = smallest_node_covering_range(tree.root_node(), self.start, self.end)?;
        if is_comment_or_string_node(node.kind()) {
            return None;
        }
        let span = NormalizedSpan::from_content_range(content, self.start, self.end)?;
        let normalized = crate::base::http_boundary::normalize_route_template(
            &self.mount_path,
            ParamFlavor::Colon,
        );
        let mut metadata = base_metadata("framework", "express");
        insert_string(&mut metadata, "mount_path", &self.mount_path);
        insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
        insert_string(&mut metadata, "mount_target", &self.mount_target);
        Some(fact_for_span(
            file_path,
            language,
            EXPRESS_ROUTER_MOUNT_PATTERN_ID,
            "router_mount",
            node.kind(),
            span,
            metadata,
        ))
    }
}

fn collect_node_imports(content: &str, mask: &SourceMask) -> NodeImports {
    let mut imports = NodeImports::default();
    collect_es_imports(content, mask, &mut imports);
    collect_require_imports(content, mask, &mut imports);
    imports
}

fn collect_es_imports(content: &str, mask: &SourceMask, imports: &mut NodeImports) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find("import") {
        let import_start = cursor + relative;
        cursor = import_start + "import".len();
        if !is_identifier_boundary(content, import_start, "import".len())
            || mask.is_string_or_comment(import_start)
        {
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
        if !matches!(source.as_str(), "express" | "fastify") {
            continue;
        }
        let Some(local) =
            parse_default_import(statement).or_else(|| parse_namespace_import(statement))
        else {
            continue;
        };
        if source == "express" {
            imports.express.insert(local);
        } else {
            imports.fastify.insert(local);
        }
    }
}

fn collect_require_imports(content: &str, mask: &SourceMask, imports: &mut NodeImports) {
    for source in ["express", "fastify"] {
        let needle = format!("require('{source}')");
        collect_require_imports_for(content, mask, &needle, source, imports);
        let needle = format!("require(\"{source}\")");
        collect_require_imports_for(content, mask, &needle, source, imports);
    }
}

fn collect_require_imports_for(
    content: &str,
    mask: &SourceMask,
    needle: &str,
    source: &str,
    imports: &mut NodeImports,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let require_start = cursor + relative;
        cursor = require_start + needle.len();
        if mask.is_string_or_comment(require_start) {
            continue;
        }
        let statement_start = content[..require_start]
            .rfind(['\n', ';'])
            .map(|index| index + 1)
            .unwrap_or(0);
        let before = content[statement_start..require_start].trim();
        let Some(local) = before
            .strip_prefix("const ")
            .or_else(|| before.strip_prefix("let "))
            .or_else(|| before.strip_prefix("var "))
            .and_then(|prefix| prefix.split('=').next())
            .map(str::trim)
            .filter(|value| is_js_identifier(value))
        else {
            continue;
        };
        if source == "express" {
            imports.express.insert(local.to_string());
        } else {
            imports.fastify.insert(local.to_string());
        }
    }
}

fn collect_express_receivers(
    content: &str,
    mask: &SourceMask,
    imports: &NodeImports,
) -> HashMap<String, ReceiverKind> {
    let mut receivers = HashMap::new();
    for local in &imports.express {
        collect_call_assignment_receivers(
            content,
            mask,
            &format!("{local}()"),
            ReceiverKind::ExpressApp,
            &mut receivers,
        );
        collect_call_assignment_receivers(
            content,
            mask,
            &format!("{local}.Router()"),
            ReceiverKind::ExpressRouter,
            &mut receivers,
        );
    }
    receivers
}

fn collect_fastify_receivers(
    content: &str,
    mask: &SourceMask,
    imports: &NodeImports,
) -> BTreeSet<String> {
    let mut receivers = BTreeSet::new();
    for local in &imports.fastify {
        collect_call_assignment_receiver_names(
            content,
            mask,
            &format!("{local}()"),
            &mut receivers,
        );
    }
    // Plugin-parameter gate: a parameter literally named `fastify` attests the
    // framework by itself; the generic `app` name is a common Express idiom
    // (`module.exports = function (app)`) and only counts when the file also
    // imports fastify.
    for plugin_param in fastify_plugin_parameters(content, mask) {
        if plugin_param == "fastify" || !imports.fastify.is_empty() {
            receivers.insert(plugin_param);
        }
    }
    receivers
}

fn collect_call_assignment_receivers(
    content: &str,
    mask: &SourceMask,
    call: &str,
    kind: ReceiverKind,
    receivers: &mut HashMap<String, ReceiverKind>,
) {
    let mut names = BTreeSet::new();
    collect_call_assignment_receiver_names(content, mask, call, &mut names);
    receivers.extend(names.into_iter().map(|name| (name, kind)));
}

fn collect_call_assignment_receiver_names(
    content: &str,
    mask: &SourceMask,
    call: &str,
    receivers: &mut BTreeSet<String>,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(call) {
        let call_start = cursor + relative;
        cursor = call_start + call.len();
        if mask.is_string_or_comment(call_start) {
            continue;
        }
        let statement_start = content[..call_start]
            .rfind(['\n', ';', '{'])
            .map(|index| index + 1)
            .unwrap_or(0);
        let before = content[statement_start..call_start].trim();
        let Some(name) = before
            .strip_prefix("const ")
            .or_else(|| before.strip_prefix("let "))
            .or_else(|| before.strip_prefix("var "))
            .and_then(|prefix| prefix.split('=').next())
            .map(str::trim)
            .filter(|value| is_js_identifier(value))
        else {
            continue;
        };
        receivers.insert(name.to_string());
    }
}

fn fastify_plugin_parameters(content: &str, mask: &SourceMask) -> Vec<String> {
    let mut params = Vec::new();
    for marker in [
        "export default async function",
        "export default function",
        "module.exports = async function",
        "module.exports = function",
    ] {
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find(marker) {
            let start = cursor + relative;
            cursor = start + marker.len();
            if mask.is_string_or_comment(start) {
                continue;
            }
            let Some(open) = content[cursor..].find('(').map(|offset| cursor + offset) else {
                continue;
            };
            let Some(close) = find_matching_paren(content, mask, open) else {
                continue;
            };
            let first = content[open + 1..close].split(',').next().map(str::trim);
            if matches!(first, Some("fastify" | "app")) {
                params.push(first.unwrap().to_string());
            }
        }
    }
    params
}

fn collect_express_mounts(
    content: &str,
    mask: &SourceMask,
    receivers: &HashMap<String, ReceiverKind>,
) -> ExpressMounts {
    let mut facts = Vec::new();
    let mut same_file_prefixes = HashMap::new();
    for receiver in receivers.keys() {
        let needle = format!("{receiver}.use");
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find(&needle) {
            let name_start = cursor + relative;
            cursor = name_start + needle.len();
            if !is_identifier_boundary(content, name_start, receiver.len())
                || mask.is_string_or_comment(name_start)
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
            let first_start = skip_ascii_whitespace_until(content, open + 1, close);
            let first_end = find_top_level_comma_or_end(content, mask, first_start, close);
            let Some((mount_path, path_end)) = parse_js_string_literal(content, first_start) else {
                continue;
            };
            if skip_ascii_whitespace_until(content, path_end, first_end) != first_end {
                continue;
            }
            let second_start = skip_ascii_whitespace_until(content, first_end + 1, close);
            if second_start >= close {
                continue;
            }
            let second_end = find_top_level_comma_or_end(content, mask, second_start, close);
            let mount_target = content[second_start..second_end].trim().to_string();
            if mount_target.is_empty()
                || mount_target.starts_with(['\'', '"'])
                || mount_target.starts_with('{')
            {
                continue;
            }
            if receivers.contains_key(&mount_target) {
                same_file_prefixes.insert(mount_target.clone(), mount_path.clone());
            }
            facts.push(MountCandidate {
                start: name_start,
                end: close + 1,
                mount_path,
                mount_target,
            });
        }
    }
    ExpressMounts {
        facts,
        same_file_prefixes,
    }
}

fn collect_express_route_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    receivers: &HashMap<String, ReceiverKind>,
    prefixes: &HashMap<String, String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for receiver in receivers.keys() {
        for (method, verb) in JS_VERB_METHODS {
            collect_route_method_calls(
                language,
                tree,
                file_path,
                content,
                mask,
                receiver,
                method,
                *verb,
                "express",
                EXPRESS_ROUTE_PATTERN_ID,
                prefixes.get(receiver).map(String::as_str),
                &mut facts,
            );
        }
    }
    facts
}

fn collect_express_route_chains(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    receivers: &HashMap<String, ReceiverKind>,
    prefixes: &HashMap<String, String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for receiver in receivers.keys() {
        let needle = format!("{receiver}.route");
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find(&needle) {
            let route_start = cursor + relative;
            cursor = route_start + needle.len();
            if !is_identifier_boundary(content, route_start, receiver.len())
                || mask.is_string_or_comment(route_start)
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
            let route_arg_start = skip_ascii_whitespace_until(content, open + 1, close);
            let Some((route_template, route_end)) =
                parse_js_string_literal(content, route_arg_start)
            else {
                continue;
            };
            if skip_ascii_whitespace_until(content, route_end, close) != close {
                continue;
            }
            // Walk the chained calls one by one; each hop jumps over the
            // chained call's argument list, so handler bodies are never
            // scanned and multi-line chains work.
            let mut chain_cursor = close + 1;
            loop {
                let dot = skip_ascii_whitespace_until(content, chain_cursor, content.len());
                if content.as_bytes().get(dot) != Some(&b'.') {
                    break;
                }
                let Some((method_name, name_end)) =
                    parse_js_identifier(content, dot + 1, content.len())
                else {
                    break;
                };
                let method_open = skip_ascii_whitespace_until(content, name_end, content.len());
                if content.as_bytes().get(method_open) != Some(&b'(') {
                    break;
                }
                let Some(method_close) =
                    find_matching_paren_within(content, mask, method_open, content.len())
                else {
                    break;
                };
                chain_cursor = method_close + 1;
                let verb = JS_VERB_METHODS
                    .iter()
                    .find(|(method, verb)| *method == method_name && verb.is_some())
                    .and_then(|(_, verb)| *verb);
                let Some(verb) = verb else {
                    continue;
                };
                let handler_start =
                    skip_ascii_whitespace_until(content, method_open + 1, method_close);
                if handler_start >= method_close {
                    continue;
                }
                if let Some(fact) = route_fact(
                    language,
                    tree,
                    file_path,
                    content,
                    route_start,
                    method_close + 1,
                    RouteFactSpec {
                        framework: "express",
                        pattern_id: EXPRESS_ROUTE_PATTERN_ID,
                        capture_name: "route_call",
                        api_style: "call_routing",
                        route_template: &route_template,
                        verb: Some(verb),
                        verb_source: Some("attested"),
                        flavor: ParamFlavor::Colon,
                        prefix: prefixes.get(receiver).map(String::as_str),
                        prefix_key: Some("route_group_prefix"),
                    },
                    |_| {},
                ) {
                    facts.push(fact);
                }
            }
        }
    }
    facts
}

fn collect_fastify_route_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    receivers: &BTreeSet<String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for receiver in receivers {
        for (method, verb) in JS_VERB_METHODS {
            collect_route_method_calls(
                language,
                tree,
                file_path,
                content,
                mask,
                receiver,
                method,
                *verb,
                "fastify",
                FASTIFY_ROUTE_PATTERN_ID,
                None,
                &mut facts,
            );
        }
    }
    facts
}

#[allow(clippy::too_many_arguments)]
fn collect_route_method_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    receiver: &str,
    method: &str,
    verb: Option<&str>,
    framework: &str,
    pattern_id: &str,
    route_group_prefix: Option<&str>,
    facts: &mut Vec<StructuralFact>,
) {
    let needle = format!("{receiver}.{method}");
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
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let first_end = find_top_level_comma_or_end(content, mask, first_start, close);
        let Some((route_template, route_end)) = parse_js_string_literal(content, first_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, route_end, first_end) != first_end {
            continue;
        }
        // Route registrations carry a handler after the path. A sole string
        // argument is Express's settings getter (`app.get('port')`), not a
        // route.
        let handler_start = skip_ascii_whitespace_until(content, first_end + 1, close);
        if handler_start >= close {
            continue;
        }
        if let Some(fact) = route_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            RouteFactSpec {
                framework,
                pattern_id,
                capture_name: "route_call",
                api_style: "call_routing",
                route_template: &route_template,
                verb,
                verb_source: verb.is_some().then_some("attested"),
                flavor: ParamFlavor::Colon,
                prefix: route_group_prefix,
                prefix_key: Some("route_group_prefix"),
            },
            |_| {},
        ) {
            facts.push(fact);
        }
    }
}

fn collect_fastify_route_objects(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    receivers: &BTreeSet<String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for receiver in receivers {
        let needle = format!("{receiver}.route");
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
            let object_start = skip_ascii_whitespace_until(content, open + 1, close);
            if content.as_bytes().get(object_start) != Some(&b'{') {
                continue;
            }
            let Some(object_end) = find_matching_brace_within(content, mask, object_start, close)
            else {
                continue;
            };
            let Some(route_template) =
                object_string_property(content, mask, object_start + 1, object_end, "url").or_else(
                    || object_string_property(content, mask, object_start + 1, object_end, "path"),
                )
            else {
                continue;
            };
            let verbs = object_method_verbs(content, mask, object_start + 1, object_end);
            let verbs: Vec<Option<String>> = if verbs.is_empty() {
                vec![None]
            } else {
                verbs.into_iter().map(Some).collect()
            };
            for verb in verbs {
                if let Some(fact) = route_fact(
                    language,
                    tree,
                    file_path,
                    content,
                    call_start,
                    close + 1,
                    RouteFactSpec {
                        framework: "fastify",
                        pattern_id: FASTIFY_ROUTE_PATTERN_ID,
                        capture_name: "route_call",
                        api_style: "call_routing",
                        route_template: &route_template,
                        verb: verb.as_deref(),
                        verb_source: verb.is_some().then_some("attested"),
                        flavor: ParamFlavor::Colon,
                        prefix: None,
                        prefix_key: Some("route_group_prefix"),
                    },
                    |_| {},
                ) {
                    facts.push(fact);
                }
            }
        }
    }
    facts
}

fn object_string_property(
    content: &str,
    mask: &SourceMask,
    start: usize,
    end: usize,
    key: &str,
) -> Option<String> {
    let value_start = object_property_value_start(content, start, end, key)?;
    let (value, value_end) = parse_js_string_literal(content, value_start)?;
    let value_end = skip_ascii_whitespace_until(
        content,
        value_end,
        find_top_level_comma_or_end(content, mask, value_start, end),
    );
    (value_end <= end).then_some(value)
}

fn object_method_verbs(content: &str, mask: &SourceMask, start: usize, end: usize) -> Vec<String> {
    let Some(value_start) = object_property_value_start(content, start, end, "method") else {
        return Vec::new();
    };
    if let Some((method, method_end)) = parse_js_string_literal(content, value_start) {
        let value_end = find_top_level_comma_or_end(content, mask, value_start, end);
        if skip_ascii_whitespace_until(content, method_end, value_end) == value_end {
            return vec![method.to_uppercase()];
        }
        return Vec::new();
    }
    if content.as_bytes().get(value_start) == Some(&b'[') {
        let Some(array_end) = find_matching_bracket_within(content, mask, value_start, end) else {
            return Vec::new();
        };
        let mut verbs = Vec::new();
        let mut cursor = value_start + 1;
        while cursor < array_end {
            cursor = skip_ascii_whitespace_until(content, cursor, array_end);
            if cursor >= array_end {
                break;
            }
            let Some((method, method_end)) = parse_js_string_literal(content, cursor) else {
                return Vec::new();
            };
            verbs.push(method.to_uppercase());
            cursor = skip_ascii_whitespace_until(content, method_end, array_end);
            if content.as_bytes().get(cursor) == Some(&b',') {
                cursor += 1;
            }
        }
        return verbs;
    }
    Vec::new()
}

fn object_property_value_start(
    content: &str,
    start: usize,
    end: usize,
    key: &str,
) -> Option<usize> {
    let mut cursor = start;
    while let Some(relative) = content[cursor..end].find(key) {
        let key_start = cursor + relative;
        cursor = key_start + key.len();
        if !is_identifier_boundary(content, key_start, key.len()) {
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

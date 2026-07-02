use std::collections::{BTreeSet, HashMap};

use serde_json::Value;
use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node, is_identifier_boundary,
    skip_ascii_whitespace_until, smallest_node_covering_range,
};
use super::{EXPRESS_ROUTE_PATTERN_ID, EXPRESS_ROUTER_MOUNT_PATTERN_ID, FASTIFY_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

const EXPRESS_VERB_METHODS: &[(&str, Option<&str>)] = &[
    ("get", Some("GET")),
    ("post", Some("POST")),
    ("put", Some("PUT")),
    ("patch", Some("PATCH")),
    ("delete", Some("DELETE")),
    ("head", Some("HEAD")),
    ("options", Some("OPTIONS")),
    ("all", None),
];

const FASTIFY_VERB_METHODS: &[(&str, Option<&str>)] = &[
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
    let imports = collect_node_imports(content);
    let express_receivers = collect_express_receivers(content, &imports);
    let fastify_receivers = collect_fastify_receivers(content, &imports);
    let express_mounts = collect_express_mounts(content, &express_receivers);

    let mut facts = Vec::new();
    facts.extend(collect_express_route_calls(
        language,
        tree,
        file_path,
        content,
        &express_receivers,
        &express_mounts.same_file_prefixes,
    ));
    facts.extend(collect_express_route_chains(
        language,
        tree,
        file_path,
        content,
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
        &fastify_receivers,
    ));
    facts.extend(collect_fastify_route_objects(
        language,
        tree,
        file_path,
        content,
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
        let normalized = normalize_route_template(&self.mount_path, ParamFlavor::Colon);
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

fn collect_node_imports(content: &str) -> NodeImports {
    let mut imports = NodeImports::default();
    collect_es_imports(content, &mut imports);
    collect_require_imports(content, &mut imports);
    imports
}

fn collect_es_imports(content: &str, imports: &mut NodeImports) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find("import") {
        let import_start = cursor + relative;
        cursor = import_start + "import".len();
        if !is_identifier_boundary(content, import_start, "import".len())
            || is_in_comment_or_string(content, import_start)
        {
            continue;
        }
        let statement_end = statement_end(content, import_start);
        let Some(statement) = content.get(import_start..statement_end) else {
            continue;
        };
        cursor = statement_end;
        let Some(source) = import_source(statement) else {
            continue;
        };
        if !matches!(source.as_str(), "express" | "fastify") {
            continue;
        }
        let Some(local) = default_or_namespace_import(statement) else {
            continue;
        };
        if source == "express" {
            imports.express.insert(local);
        } else {
            imports.fastify.insert(local);
        }
    }
}

fn collect_require_imports(content: &str, imports: &mut NodeImports) {
    for source in ["express", "fastify"] {
        let needle = format!("require('{source}')");
        collect_require_imports_for(content, &needle, source, imports);
        let needle = format!("require(\"{source}\")");
        collect_require_imports_for(content, &needle, source, imports);
    }
}

fn collect_require_imports_for(
    content: &str,
    needle: &str,
    source: &str,
    imports: &mut NodeImports,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let require_start = cursor + relative;
        cursor = require_start + needle.len();
        if is_in_comment_or_string(content, require_start) {
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
    imports: &NodeImports,
) -> HashMap<String, ReceiverKind> {
    let mut receivers = HashMap::new();
    for local in &imports.express {
        collect_call_assignment_receivers(
            content,
            &format!("{local}()"),
            ReceiverKind::ExpressApp,
            &mut receivers,
        );
        collect_call_assignment_receivers(
            content,
            &format!("{local}.Router()"),
            ReceiverKind::ExpressRouter,
            &mut receivers,
        );
    }
    receivers
}

fn collect_fastify_receivers(content: &str, imports: &NodeImports) -> BTreeSet<String> {
    let mut receivers = BTreeSet::new();
    for local in &imports.fastify {
        collect_call_assignment_receiver_names(content, &format!("{local}()"), &mut receivers);
    }
    for plugin_param in fastify_plugin_parameters(content) {
        receivers.insert(plugin_param);
    }
    receivers
}

fn collect_call_assignment_receivers(
    content: &str,
    call: &str,
    kind: ReceiverKind,
    receivers: &mut HashMap<String, ReceiverKind>,
) {
    let mut names = BTreeSet::new();
    collect_call_assignment_receiver_names(content, call, &mut names);
    receivers.extend(names.into_iter().map(|name| (name, kind)));
}

fn collect_call_assignment_receiver_names(
    content: &str,
    call: &str,
    receivers: &mut BTreeSet<String>,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(call) {
        let call_start = cursor + relative;
        cursor = call_start + call.len();
        if is_in_comment_or_string(content, call_start) {
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

fn fastify_plugin_parameters(content: &str) -> Vec<String> {
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
            if is_in_comment_or_string(content, start) {
                continue;
            }
            let Some(open) = content[cursor..].find('(').map(|offset| cursor + offset) else {
                continue;
            };
            let Some(close) = find_matching_paren(content, open, content.len()) else {
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
                || is_in_comment_or_string(content, name_start)
            {
                continue;
            }
            let open = skip_ascii_whitespace_until(content, cursor, content.len());
            if content.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let Some(close) = find_matching_paren(content, open, content.len()) else {
                continue;
            };
            let first_start = skip_ascii_whitespace_until(content, open + 1, close);
            let first_end = find_top_level_comma_or_end(content, first_start, close);
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
            let second_end = find_top_level_comma_or_end(content, second_start, close);
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
    receivers: &HashMap<String, ReceiverKind>,
    prefixes: &HashMap<String, String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for receiver in receivers.keys() {
        for (method, verb) in EXPRESS_VERB_METHODS {
            collect_route_method_calls(
                language,
                tree,
                file_path,
                content,
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
                || is_in_comment_or_string(content, route_start)
            {
                continue;
            }
            let open = skip_ascii_whitespace_until(content, cursor, content.len());
            if content.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let Some(close) = find_matching_paren(content, open, content.len()) else {
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
            let chain_end = statement_end(content, close + 1);
            for (method, verb) in EXPRESS_VERB_METHODS
                .iter()
                .filter(|(_, verb)| verb.is_some())
            {
                let chain_needle = format!(".{method}");
                let mut chain_cursor = close + 1;
                while let Some(chain_relative) =
                    content[chain_cursor..chain_end].find(&chain_needle)
                {
                    let method_start = chain_cursor + chain_relative + 1;
                    chain_cursor = method_start + method.len();
                    let method_open = skip_ascii_whitespace_until(content, chain_cursor, chain_end);
                    if content.as_bytes().get(method_open) != Some(&b'(') {
                        continue;
                    }
                    let Some(method_close) = find_matching_paren(content, method_open, chain_end)
                    else {
                        continue;
                    };
                    if let Some(fact) = route_fact(
                        language,
                        tree,
                        file_path,
                        content,
                        route_start,
                        method_close + 1,
                        "express",
                        EXPRESS_ROUTE_PATTERN_ID,
                        "route_call",
                        &route_template,
                        *verb,
                        "call_routing",
                        ParamFlavor::Colon,
                        prefixes.get(receiver).map(String::as_str),
                    ) {
                        facts.push(fact);
                    }
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
    receivers: &BTreeSet<String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for receiver in receivers {
        for (method, verb) in FASTIFY_VERB_METHODS {
            collect_route_method_calls(
                language,
                tree,
                file_path,
                content,
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
        if !is_identifier_boundary(content, call_start, receiver.len())
            || is_in_comment_or_string(content, call_start)
        {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, open, content.len()) else {
            continue;
        };
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let first_end = find_top_level_comma_or_end(content, first_start, close);
        let Some((route_template, route_end)) = parse_js_string_literal(content, first_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, route_end, first_end) != first_end {
            continue;
        }
        if let Some(fact) = route_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            framework,
            pattern_id,
            "route_call",
            &route_template,
            verb,
            "call_routing",
            ParamFlavor::Colon,
            route_group_prefix,
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
    receivers: &BTreeSet<String>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for receiver in receivers {
        let needle = format!("{receiver}.route");
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find(&needle) {
            let call_start = cursor + relative;
            cursor = call_start + needle.len();
            if !is_identifier_boundary(content, call_start, receiver.len())
                || is_in_comment_or_string(content, call_start)
            {
                continue;
            }
            let open = skip_ascii_whitespace_until(content, cursor, content.len());
            if content.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let Some(close) = find_matching_paren(content, open, content.len()) else {
                continue;
            };
            let object_start = skip_ascii_whitespace_until(content, open + 1, close);
            if content.as_bytes().get(object_start) != Some(&b'{') {
                continue;
            }
            let Some(object_end) = find_matching_brace(content, object_start, close) else {
                continue;
            };
            let Some(route_template) =
                object_string_property(content, object_start + 1, object_end, "url").or_else(
                    || object_string_property(content, object_start + 1, object_end, "path"),
                )
            else {
                continue;
            };
            let verbs = object_method_verbs(content, object_start + 1, object_end);
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
                    "fastify",
                    FASTIFY_ROUTE_PATTERN_ID,
                    "route_call",
                    &route_template,
                    verb.as_deref(),
                    "call_routing",
                    ParamFlavor::Colon,
                    None,
                ) {
                    facts.push(fact);
                }
            }
        }
    }
    facts
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
    capture_name: &str,
    route_template: &str,
    verb: Option<&str>,
    api_style: &str,
    flavor: ParamFlavor,
    route_group_prefix: Option<&str>,
) -> Option<StructuralFact> {
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let mut metadata = base_metadata("framework", framework);
    insert_string(&mut metadata, "api_style", api_style);
    insert_string(&mut metadata, "route_template", route_template);

    let mut normalized_source = route_template.to_string();
    if let Some(prefix) = route_group_prefix {
        let effective = join_route_templates(prefix, route_template);
        insert_string(&mut metadata, "route_group_prefix", prefix);
        insert_string(&mut metadata, "effective_route_template", &effective);
        normalized_source = effective;
    }
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
        capture_name,
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

fn object_string_property(content: &str, start: usize, end: usize, key: &str) -> Option<String> {
    let value_start = object_property_value_start(content, start, end, key)?;
    let (value, value_end) = parse_js_string_literal(content, value_start)?;
    let value_end = skip_ascii_whitespace_until(
        content,
        value_end,
        find_top_level_comma_or_end(content, value_start, end),
    );
    (value_end <= end).then_some(value)
}

fn object_method_verbs(content: &str, start: usize, end: usize) -> Vec<String> {
    let Some(value_start) = object_property_value_start(content, start, end, "method") else {
        return Vec::new();
    };
    if let Some((method, method_end)) = parse_js_string_literal(content, value_start) {
        let value_end = find_top_level_comma_or_end(content, value_start, end);
        if skip_ascii_whitespace_until(content, method_end, value_end) == value_end {
            return vec![method.to_uppercase()];
        }
        return Vec::new();
    }
    if content.as_bytes().get(value_start) == Some(&b'[') {
        let Some(array_end) = find_matching_bracket(content, value_start, end) else {
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

fn parse_js_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let quote = content
        .as_bytes()
        .get(start)
        .copied()
        .filter(|byte| matches!(*byte, b'\'' | b'"'))?;
    let mut cursor = start + 1;
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
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

fn import_source(statement: &str) -> Option<String> {
    let from_start = statement.rfind("from")?;
    if !is_identifier_boundary(statement, from_start, "from".len()) {
        return None;
    }
    let source_start =
        skip_ascii_whitespace_until(statement, from_start + "from".len(), statement.len());
    let (source, _) = parse_js_string_literal(statement, source_start)?;
    Some(source)
}

fn default_or_namespace_import(statement: &str) -> Option<String> {
    let after_import = skip_ascii_whitespace_until(statement, "import".len(), statement.len());
    if statement.as_bytes().get(after_import) == Some(&b'*') {
        let as_start = skip_ascii_whitespace_until(statement, after_import + 1, statement.len());
        if !statement[as_start..].starts_with("as")
            || !is_identifier_boundary(statement, as_start, "as".len())
        {
            return None;
        }
        let local_start =
            skip_ascii_whitespace_until(statement, as_start + "as".len(), statement.len());
        return parse_js_identifier(statement, local_start, statement.len()).map(|(name, _)| name);
    }
    if matches!(statement.as_bytes().get(after_import), Some(b'{')) {
        return None;
    }
    parse_js_identifier(statement, after_import, statement.len()).map(|(name, _)| name)
}

fn parse_js_identifier(content: &str, start: usize, end: usize) -> Option<(String, usize)> {
    let first = *content.as_bytes().get(start)?;
    if !is_js_identifier_start_byte(first) {
        return None;
    }
    let mut cursor = start + 1;
    while cursor < end
        && content
            .as_bytes()
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

fn is_js_identifier_start_byte(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_js_identifier_byte(byte: u8) -> bool {
    is_js_identifier_start_byte(byte) || byte.is_ascii_digit()
}

fn statement_end(content: &str, start: usize) -> usize {
    let bytes = content.as_bytes();
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
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
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        } else {
            match byte {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b';' | b'\n' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                    return cursor + 1;
                }
                _ => {}
            }
        }
        cursor += 1;
    }
    content.len()
}

fn find_matching_paren(content: &str, open: usize, end: usize) -> Option<usize> {
    find_matching_delimiter(content, open, end, b'(', b')')
}

fn find_matching_brace(content: &str, open: usize, end: usize) -> Option<usize> {
    find_matching_delimiter(content, open, end, b'{', b'}')
}

fn find_matching_bracket(content: &str, open: usize, end: usize) -> Option<usize> {
    find_matching_delimiter(content, open, end, b'[', b']')
}

fn find_matching_delimiter(
    content: &str,
    open: usize,
    end: usize,
    left: u8,
    right: u8,
) -> Option<usize> {
    if content.as_bytes().get(open) != Some(&left) {
        return None;
    }
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut cursor = open;
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
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        } else if byte == left {
            depth += 1;
        } else if byte == right {
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
    let bytes = content.as_bytes();
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
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
        } else if matches!(byte, b'\'' | b'"' | b'`') {
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

fn is_in_comment_or_string(content: &str, target: usize) -> bool {
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
            if escaped {
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
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        }
        cursor += 1;
    }
    line_comment || block_comment || quote.is_some()
}

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::Tree;

use super::super::helpers::{
    is_ascii_identifier, is_identifier_boundary, skip_ascii_whitespace_until,
};
use super::super::scan::{
    MaskLanguage, SourceMask, find_matching_paren, find_top_level_comma_or_end,
    parse_python_string_literal,
};
use super::client_fact;
use crate::base::types::StructuralFact;

const CLIENT_METHODS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("head", "HEAD"),
    ("options", "OPTIONS"),
];

pub(super) fn collect_python_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports = collect_python_http_client_imports(content);
    if imports.is_empty() {
        return Vec::new();
    }
    let mask = SourceMask::new(content, MaskLanguage::Python);
    let mut facts = Vec::new();
    let whole_file = 0..content.len();
    for (local, client) in &imports.modules {
        collect_method_requests(
            language,
            tree,
            file_path,
            content,
            &mask,
            local,
            client,
            &whole_file,
            &mut facts,
        );
        collect_request_calls(
            language,
            tree,
            file_path,
            content,
            &mask,
            local,
            client,
            &whole_file,
            &mut facts,
        );
    }
    for (local, (client, verb)) in &imports.verb_functions {
        collect_bare_verb_calls(
            language, tree, file_path, content, &mask, local, client, verb, &mut facts,
        );
    }
    for receiver in collect_client_instances(tree, content, &mask, &imports) {
        collect_method_requests(
            language,
            tree,
            file_path,
            content,
            &mask,
            &receiver.name,
            &receiver.client,
            &receiver.scope,
            &mut facts,
        );
        collect_request_calls(
            language,
            tree,
            file_path,
            content,
            &mask,
            &receiver.name,
            &receiver.client,
            &receiver.scope,
            &mut facts,
        );
    }
    facts
}

#[derive(Default)]
struct PythonHttpClientImports {
    /// `import requests [as r]`: local module name to client.
    modules: HashMap<String, String>,
    /// `from requests import get [as g]`: local function name to (client, verb).
    verb_functions: HashMap<String, (String, &'static str)>,
    /// `from httpx import Client`: local class name to client.
    client_classes: HashMap<String, String>,
}

impl PythonHttpClientImports {
    fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.verb_functions.is_empty() && self.client_classes.is_empty()
    }
}

const CLIENT_CLASSES: &[&str] = &["Session", "Client", "AsyncClient"];

fn collect_python_http_client_imports(content: &str) -> PythonHttpClientImports {
    let mut imports = PythonHttpClientImports::default();
    for line in content.lines() {
        let trimmed = line.trim();
        let trimmed = trimmed.split('#').next().unwrap_or(trimmed);
        if let Some(rest) = trimmed.strip_prefix("import ") {
            for item in rest.split(',') {
                let mut parts = item.split_whitespace();
                let Some(module @ ("requests" | "httpx")) = parts.next() else {
                    continue;
                };
                let local = if parts.next() == Some("as") {
                    parts.next().filter(|alias| is_ascii_identifier(alias))
                } else {
                    Some(module)
                };
                if let Some(local) = local {
                    imports
                        .modules
                        .insert(local.to_string(), module.to_string());
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("from ")
            && let Some((module @ ("requests" | "httpx"), items)) = rest
                .split_once(" import ")
                .map(|(module, items)| (module.trim(), items))
        {
            for item in items.trim_matches(['(', ')', ' ']).split(',') {
                let mut parts = item.split_whitespace();
                let Some(imported) = parts.next() else {
                    continue;
                };
                let local = if parts.next() == Some("as") {
                    parts.next().unwrap_or(imported)
                } else {
                    imported
                };
                if !is_ascii_identifier(local) {
                    continue;
                }
                if let Some((_, verb)) = CLIENT_METHODS.iter().find(|(name, _)| *name == imported) {
                    imports
                        .verb_functions
                        .insert(local.to_string(), (module.to_string(), verb));
                } else if CLIENT_CLASSES.contains(&imported) {
                    imports
                        .client_classes
                        .insert(local.to_string(), module.to_string());
                }
            }
        }
    }
    imports
}

struct ClientInstance {
    name: String,
    client: String,
    scope: Range<usize>,
}

/// Local names bound to a client constructed in this file: `s =
/// requests.Session()`, `client = httpx.Client(...)`, or `with
/// httpx.AsyncClient(...) as client:`. Each name is a receiver only inside the
/// function (or module) that binds it.
fn collect_client_instances(
    tree: &Tree,
    content: &str,
    mask: &SourceMask,
    imports: &PythonHttpClientImports,
) -> Vec<ClientInstance> {
    let mut constructors: Vec<(String, String)> = imports
        .client_classes
        .iter()
        .map(|(local, client)| (local.clone(), client.clone()))
        .collect();
    for (local, client) in &imports.modules {
        for class in CLIENT_CLASSES {
            constructors.push((format!("{local}.{class}"), client.clone()));
        }
    }
    let mut instances = Vec::new();
    for (constructor, client) in constructors {
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find(&constructor) {
            let start = cursor + relative;
            cursor = start + constructor.len();
            let head_len = constructor.split('.').next().unwrap_or(&constructor).len();
            if !is_identifier_boundary(content, start, head_len)
                || content[..start].ends_with('.')
                || mask.is_string_or_comment(start)
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
            let Some(name) = assigned_name(content, start).or_else(|| with_as_name(content, close))
            else {
                continue;
            };
            instances.push(ClientInstance {
                name,
                client: client.clone(),
                scope: enclosing_scope(tree, start),
            });
        }
    }
    instances
}

/// `name` in `name = <call>` or `name: T = <call>` on the call's line.
fn assigned_name(content: &str, call_start: usize) -> Option<String> {
    let line_start = content[..call_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let target = content[line_start..call_start].trim().strip_suffix('=')?;
    let name = target.split(':').next()?.trim();
    is_ascii_identifier(name).then(|| name.to_string())
}

/// `name` in `<call> as name`.
fn with_as_name(content: &str, close: usize) -> Option<String> {
    let rest = content[close + 1..].trim_start().strip_prefix("as")?;
    let rest = rest
        .strip_prefix(|c: char| c.is_ascii_whitespace())?
        .trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let name = &rest[..end];
    is_ascii_identifier(name).then(|| name.to_string())
}

/// The byte range of the innermost function around `byte`, or the whole file.
fn enclosing_scope(tree: &Tree, byte: usize) -> Range<usize> {
    let mut node = tree.root_node().descendant_for_byte_range(byte, byte);
    while let Some(current) = node {
        if matches!(
            current.kind(),
            "function_definition" | "async_function_definition" | "lambda"
        ) {
            return current.start_byte()..current.end_byte();
        }
        node = current.parent();
    }
    let root = tree.root_node();
    root.start_byte()..root.end_byte()
}

/// `get("https://...")` where `get` is imported from requests or httpx.
#[allow(clippy::too_many_arguments)]
fn collect_bare_verb_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    local: &str,
    client: &str,
    verb: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(local) {
        let call_start = cursor + relative;
        cursor = call_start + local.len();
        if !is_identifier_boundary(content, call_start, local.len())
            || content[..call_start].trim_end().ends_with(['.', '@'])
            || content[..call_start].trim_end().ends_with("def")
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
        let Some(target_path) = url_argument(content, mask, open, close) else {
            continue;
        };
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            client,
            &target_path,
            verb,
            "attested",
            Some(client),
        ) {
            facts.push(fact);
        }
    }
}

/// The request URL of a client call: a string literal first argument, else a
/// string literal `url=` keyword argument.
fn url_argument(content: &str, mask: &SourceMask, open: usize, close: usize) -> Option<String> {
    let first_start = skip_ascii_whitespace_until(content, open + 1, close);
    let first_end = find_top_level_comma_or_end(content, mask, first_start, close);
    if let Some((target_path, url_end)) = parse_python_string_literal(content, first_start)
        && skip_ascii_whitespace_until(content, url_end, first_end) == first_end
    {
        return Some(target_path);
    }
    let mut arg_start = first_start;
    while arg_start < close {
        let arg_end = find_top_level_comma_or_end(content, mask, arg_start, close);
        let argument = &content[arg_start..arg_end];
        if let Some(rest) = argument.strip_prefix("url")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let literal_start =
                skip_ascii_whitespace_until(content, arg_end - value.len(), arg_end);
            return parse_python_string_literal(content, literal_start)
                .filter(|(_, end)| skip_ascii_whitespace_until(content, *end, arg_end) == arg_end)
                .map(|(value, _)| value);
        }
        arg_start = skip_ascii_whitespace_until(content, arg_end + 1, close);
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn collect_method_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    local: &str,
    client: &str,
    scope: &Range<usize>,
    facts: &mut Vec<StructuralFact>,
) {
    for (method, verb) in CLIENT_METHODS {
        let needle = format!("{local}.{method}");
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find(&needle) {
            let call_start = cursor + relative;
            cursor = call_start + needle.len();
            if !scope.contains(&call_start)
                || !is_identifier_boundary(content, call_start, local.len())
                || content[..call_start].ends_with('.')
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
            let Some(target_path) = url_argument(content, mask, open, close) else {
                continue;
            };
            if let Some(fact) = client_fact(
                language,
                tree,
                file_path,
                content,
                call_start,
                close + 1,
                client,
                &target_path,
                verb,
                "attested",
                Some(client),
            ) {
                facts.push(fact);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_request_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    local: &str,
    client: &str,
    scope: &Range<usize>,
    facts: &mut Vec<StructuralFact>,
) {
    let needle = format!("{local}.request");
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !scope.contains(&call_start)
            || !is_identifier_boundary(content, call_start, local.len())
            || content[..call_start].ends_with('.')
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
        let method_start = skip_ascii_whitespace_until(content, open + 1, close);
        let method_end = find_top_level_comma_or_end(content, mask, method_start, close);
        let Some((method, method_literal_end)) = parse_python_string_literal(content, method_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, method_literal_end, method_end) != method_end {
            continue;
        }
        let url_start = skip_ascii_whitespace_until(content, method_end + 1, close);
        let url_end = find_top_level_comma_or_end(content, mask, url_start, close);
        let Some((target_path, target_literal_end)) =
            parse_python_string_literal(content, url_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, target_literal_end, url_end) != url_end {
            continue;
        }
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            client,
            &target_path,
            &method.to_uppercase(),
            "attested",
            Some(client),
        ) {
            facts.push(fact);
        }
    }
}

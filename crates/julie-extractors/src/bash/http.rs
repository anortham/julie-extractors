//! `curl` and `wget` argument parsing: which arguments are request URLs, and
//! which HTTP verb the flags select.

use super::invocations::{
    CommandScope, arguments, invocations, static_command_name, static_word, text,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::{Node, Tree};

pub(crate) const HTTP_CLIENTS: &[&str] = &["curl", "wget"];

const CURL_SHORT_VALUE_FLAGS: &str = "AbcCdDeEFHKmoPQrTuUwxXyYz";
const CURL_LONG_VALUE_FLAGS: &[&str] = &[
    "--header",
    "--data",
    "--data-raw",
    "--data-binary",
    "--data-urlencode",
    "--data-ascii",
    "--json",
    "--form",
    "--form-string",
    "--user",
    "--output",
    "--output-dir",
    "--user-agent",
    "--referer",
    "--cookie",
    "--cookie-jar",
    "--request",
    "--upload-file",
    "--write-out",
    "--proxy",
    "--max-time",
    "--connect-timeout",
    "--cacert",
    "--cert",
    "--key",
    "--config",
    "--retry",
    "--retry-delay",
    "--retry-max-time",
    "--resolve",
    "--oauth2-bearer",
    "--url",
    "--range",
    "--limit-rate",
    "--interface",
    "--unix-socket",
    "--variable",
];
const CURL_DATA_FLAGS: &[&str] = &[
    "-d",
    "-F",
    "--data",
    "--data-raw",
    "--data-binary",
    "--data-urlencode",
    "--data-ascii",
    "--json",
    "--form",
    "--form-string",
];

const WGET_SHORT_VALUE_FLAGS: &str = "OoaePUtTwiBQlDAR";
const WGET_LONG_VALUE_FLAGS: &[&str] = &[
    "--output-document",
    "--output-file",
    "--append-output",
    "--execute",
    "--directory-prefix",
    "--user-agent",
    "--tries",
    "--timeout",
    "--wait",
    "--input-file",
    "--base",
    "--header",
    "--post-data",
    "--post-file",
    "--body-data",
    "--body-file",
    "--method",
    "--user",
    "--password",
    "--http-user",
    "--http-password",
    "--load-cookies",
    "--save-cookies",
    "--referer",
    "--ca-certificate",
    "--certificate",
    "--private-key",
    "--limit-rate",
];

/// The request-shaped arguments of one `curl` or `wget` call.
pub(crate) struct HttpArguments<'a> {
    /// Arguments that are not flags or flag values, in order.
    pub(crate) positionals: Vec<Node<'a>>,
    pub(crate) verb: String,
    pub(crate) verb_source: &'static str,
}

pub(crate) fn http_arguments<'a>(
    content: &str,
    client: &str,
    args: &[Node<'a>],
) -> HttpArguments<'a> {
    let (short_values, long_values) = match client {
        "wget" => (WGET_SHORT_VALUE_FLAGS, WGET_LONG_VALUE_FLAGS),
        _ => (CURL_SHORT_VALUE_FLAGS, CURL_LONG_VALUE_FLAGS),
    };
    let mut positionals = Vec::new();
    let mut flags: Vec<(String, Option<String>)> = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        let word = text(content, *arg);
        let value_at = |index: usize| args.get(index).map(|next| text(content, *next).to_string());
        if let Some(long) = word.strip_prefix("--").filter(|long| !long.is_empty()) {
            let (flag, inline) = match long.split_once('=') {
                Some((flag, value)) => (format!("--{flag}"), Some(value.to_string())),
                None => (word.to_string(), None),
            };
            let value = if inline.is_none() && long_values.contains(&flag.as_str()) {
                index += 1;
                value_at(index - 1)
            } else {
                inline
            };
            if flag == "--url" && !long.contains('=') {
                positionals.extend(args.get(index - 1).copied());
            }
            flags.push((flag, value));
        } else if word.len() > 1 && word.starts_with('-') && arg.kind() == "word" {
            for (offset, letter) in word.char_indices().skip(1) {
                let flag = format!("-{letter}");
                if short_values.contains(letter) {
                    let attached = &word[offset + letter.len_utf8()..];
                    let value = if attached.is_empty() {
                        index += 1;
                        value_at(index - 1)
                    } else {
                        Some(attached.to_string())
                    };
                    flags.push((flag, value));
                    break;
                }
                flags.push((flag, None));
            }
        } else {
            positionals.push(*arg);
        }
    }
    let (verb, verb_source) = match client {
        "wget" => wget_verb(&flags),
        _ => curl_verb(&flags),
    };
    HttpArguments {
        positionals,
        verb,
        verb_source,
    }
}

fn has_flag(flags: &[(String, Option<String>)], names: &[&str]) -> bool {
    flags.iter().any(|(flag, _)| names.contains(&flag.as_str()))
}

fn flag_value<'f>(flags: &'f [(String, Option<String>)], names: &[&str]) -> Option<&'f str> {
    flags
        .iter()
        .rev()
        .find(|(flag, _)| names.contains(&flag.as_str()))
        .and_then(|(_, value)| value.as_deref())
}

fn attested_verb(value: &str) -> Option<(String, &'static str)> {
    let verb = value.trim_matches(['"', '\'']).to_ascii_uppercase();
    (!verb.is_empty() && verb.chars().all(|c| c.is_ascii_alphabetic()))
        .then_some((verb, "attested"))
}

fn curl_verb(flags: &[(String, Option<String>)]) -> (String, &'static str) {
    if let Some(verb) = flag_value(flags, &["-X", "--request"]).and_then(attested_verb) {
        return verb;
    }
    let attested = |verb: &str| (verb.to_string(), "attested");
    if has_flag(flags, &["-I", "--head"]) {
        attested("HEAD")
    } else if has_flag(flags, &["-G", "--get"]) {
        attested("GET")
    } else if has_flag(flags, &["-T", "--upload-file"]) {
        attested("PUT")
    } else if has_flag(flags, CURL_DATA_FLAGS) {
        attested("POST")
    } else {
        ("GET".to_string(), "default")
    }
}

fn wget_verb(flags: &[(String, Option<String>)]) -> (String, &'static str) {
    if let Some(verb) = flag_value(flags, &["--method"]).and_then(attested_verb) {
        return verb;
    }
    if has_flag(flags, &["--post-data", "--post-file"]) {
        ("POST".to_string(), "attested")
    } else {
        ("GET".to_string(), "default")
    }
}

/// The URL of a request argument: a static word or string that names a
/// scheme URL, an absolute path, or a host.
pub(crate) fn request_url(content: &str, node: Node<'_>) -> Option<String> {
    let url = static_word(content, node)?;
    let host = url.split('/').next().unwrap_or_default();
    (url.contains("://") || url.starts_with('/') || host.contains('.') || host.contains(':'))
        .then_some(url)
}

/// One `curl` or `wget` request with a static URL.
pub(crate) struct HttpRequest {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) client: &'static str,
    pub(crate) url: String,
    pub(crate) verb: String,
    pub(crate) verb_source: &'static str,
}

/// Every `curl`/`wget` request in `tree`, including wrapped ones (`sudo curl`).
pub(crate) fn http_requests(tree: &Tree, content: &str, test_context: bool) -> Vec<HttpRequest> {
    let local_functions = HashSet::new();
    let scope = CommandScope {
        local_functions: &local_functions,
        test_context,
    };
    let mut requests = Vec::new();
    collect_requests(tree.root_node(), content, &scope, &mut requests, 0);
    requests
}

fn collect_requests(
    node: Node<'_>,
    content: &str,
    scope: &CommandScope<'_>,
    requests: &mut Vec<HttpRequest>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "command"
        && let Some(request) = command_request(node, content, scope)
    {
        requests.push(request);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_requests(child, content, scope, requests, child_depth);
    }
}

fn command_request(node: Node<'_>, content: &str, scope: &CommandScope<'_>) -> Option<HttpRequest> {
    let (_, name) = static_command_name(content, node)?;
    let (client, args) = match HTTP_CLIENTS.iter().find(|client| **client == name) {
        Some(client) => (*client, arguments(node)),
        None => {
            let invocation = invocations(content, node, scope).into_iter().next()?;
            let client = HTTP_CLIENTS
                .iter()
                .find(|client| **client == invocation.name)?;
            (*client, invocation.arguments?)
        }
    };
    let parsed = http_arguments(content, client, &args);
    let url = request_url(content, *parsed.positionals.first()?)?;
    Some(HttpRequest {
        start: node.start_byte(),
        end: node.end_byte(),
        client,
        url,
        verb: parsed.verb,
        verb_source: parsed.verb_source,
    })
}

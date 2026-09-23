//! Dart HTTP client requests: package:http verb calls (through the import
//! prefix, or bare when the import has none) and Dio verb calls on a receiver
//! named `dio`, in a file that imports the package. The URL is a static string,
//! `Uri.parse`/`Uri.tryParse` of one, or `Uri.https`/`Uri.http` of a static
//! authority and path.
use tree_sitter::{Node, Tree};

use super::super::dart::{dart_arguments, dart_imports, dart_method_call, dart_static_string};
use super::client_fact;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const VERBS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("head", "HEAD"),
];

/// How a file reaches package:http: through an `as` prefix, or bare.
enum HttpImport {
    Absent,
    Bare,
    Prefixed(String),
}

pub(super) fn collect_dart_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports = dart_imports(tree.root_node(), content);
    let http = imports
        .iter()
        .find(|import| import.uri == "package:http/http.dart")
        .map_or(HttpImport::Absent, |import| match &import.prefix {
            Some(prefix) => HttpImport::Prefixed(prefix.clone()),
            None => HttpImport::Bare,
        });
    let dio = imports
        .iter()
        .any(|import| import.uri.starts_with("package:dio/"));
    if matches!(http, HttpImport::Absent) && !dio {
        return Vec::new();
    }
    let scan = Scan {
        language,
        tree,
        file_path,
        content,
        http,
        dio,
    };
    let mut facts = Vec::new();
    scan.walk(tree.root_node(), 0, &mut facts);
    facts
}

struct Scan<'a> {
    language: &'a str,
    tree: &'a Tree,
    file_path: &'a str,
    content: &'a str,
    http: HttpImport,
    dio: bool,
}

impl Scan<'_> {
    fn walk(&self, node: Node, depth: u32, facts: &mut Vec<StructuralFact>) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        if node.kind() == "call_expression"
            && let Some((client, import_source, method, url)) = self.client_call(node)
            && let Some(verb) = VERBS
                .iter()
                .find(|(name, _)| *name == method)
                .map(|(_, verb)| *verb)
            && let Some(target) = url_target(url, self.content)
            && let Some(fact) = client_fact(
                self.language,
                self.tree,
                self.file_path,
                self.content,
                node.start_byte(),
                node.end_byte(),
                client,
                &target,
                verb,
                "attested",
                Some(import_source),
            )
        {
            facts.push(fact);
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, child_depth, facts);
        }
    }

    /// The client, import source, method name, and URL argument of a call.
    fn client_call<'t>(
        &self,
        call: Node<'t>,
    ) -> Option<(&'static str, &'static str, &'t str, Node<'t>)>
    where
        Self: 't,
    {
        let content: &'t str = self.content;
        let arguments = call.child_by_field_name("arguments")?;
        let url = *dart_arguments(arguments, content).0.first()?;
        if let Some((receiver, method)) = dart_method_call(call, content) {
            let receiver_text = content.get(receiver.start_byte()..receiver.end_byte())?;
            if let HttpImport::Prefixed(prefix) = &self.http
                && receiver_text == prefix
            {
                return Some(("dart_http", "package:http", method, url));
            }
            if self.dio && receiver_text.to_ascii_lowercase().ends_with("dio") {
                return Some(("dio", "package:dio", method, url));
            }
            return None;
        }
        let function = call.child_by_field_name("function")?;
        if matches!(self.http, HttpImport::Bare) && function.kind() == "identifier" {
            let method = content.get(function.start_byte()..function.end_byte())?;
            return Some(("dart_http", "package:http", method, url));
        }
        None
    }
}

fn url_target(node: Node, content: &str) -> Option<String> {
    if let Some(url) = dart_static_string(node, content) {
        return Some(url.to_string());
    }
    let (receiver, method) = dart_method_call(node, content)?;
    if content.get(receiver.start_byte()..receiver.end_byte())? != "Uri" {
        return None;
    }
    let (positional, _) = dart_arguments(node.child_by_field_name("arguments")?, content);
    let static_argument = |index: usize| {
        positional
            .get(index)
            .and_then(|argument| dart_static_string(*argument, content))
    };
    match method {
        "parse" | "tryParse" => static_argument(0).map(str::to_string),
        "https" | "http" => {
            let authority = static_argument(0)?;
            let path = static_argument(1).unwrap_or("");
            let separator = if path.is_empty() || path.starts_with('/') {
                ""
            } else {
                "/"
            };
            Some(format!("{method}://{authority}{separator}{path}"))
        }
        _ => None,
    }
}

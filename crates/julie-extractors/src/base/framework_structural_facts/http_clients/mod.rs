mod csharp;
mod elixir;
mod go;
mod java;
mod kotlin;
mod php;
mod python;
mod ruby;

use tree_sitter::Tree;

use super::HTTP_CLIENT_REQUEST_PATTERN_ID;
use super::helpers::{fact_for_span, is_comment_or_string_node, smallest_node_covering_range};
use crate::base::http_boundary::client_request_metadata;
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

pub(super) fn collect_backend_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    match language {
        "python" => python::collect_python_http_client_requests(language, tree, file_path, content),
        "csharp" => csharp::collect_csharp_http_client_requests(language, tree, file_path, content),
        "go" => go::collect_go_http_client_requests(language, tree, file_path, content),
        "java" => java::collect_java_http_client_requests(language, tree, file_path, content),
        "kotlin" => {
            kotlin::collect_kotlin_http_client_requests(language, tree, file_path, content)
        }
        "php" => php::collect_php_http_client_requests(language, tree, file_path, content),
        "ruby" => ruby::collect_ruby_http_client_requests(language, tree, file_path, content),
        "elixir" => {
            elixir::collect_elixir_http_client_requests(language, tree, file_path, content)
        }
        _ => Vec::new(),
    }
}

/// Shared `http.client_request.v1` fact builder for the five backend-language
/// client collectors. The span covers the detected call; the anchoring node is
/// the smallest parser node covering that span.
#[allow(clippy::too_many_arguments)]
fn client_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    start: usize,
    end: usize,
    client: &str,
    target_path: &str,
    verb: &str,
    verb_source: &str,
    import_source: Option<&str>,
) -> Option<StructuralFact> {
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    Some(fact_for_span(
        file_path,
        language,
        HTTP_CLIENT_REQUEST_PATTERN_ID,
        "client_request",
        node.kind(),
        span,
        client_request_metadata(client, target_path, verb, verb_source, import_source),
    ))
}

mod csharp;
mod go;
mod java;
mod python;
mod ruby;

use tree_sitter::Tree;

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
        "ruby" => ruby::collect_ruby_http_client_requests(language, tree, file_path, content),
        _ => Vec::new(),
    }
}

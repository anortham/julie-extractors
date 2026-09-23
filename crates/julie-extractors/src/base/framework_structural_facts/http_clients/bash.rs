use tree_sitter::Tree;

use super::client_fact;
use crate::base::types::StructuralFact;

/// `curl` and `wget` requests with a static URL, including wrapped ones.
pub(super) fn collect_bash_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains("curl") && !content.contains("wget") {
        return Vec::new();
    }
    let test_context = crate::test_detection::is_test_path(file_path);
    crate::bash::http_requests(tree, content, test_context)
        .into_iter()
        .filter_map(|request| {
            client_fact(
                language,
                tree,
                file_path,
                content,
                request.start,
                request.end,
                request.client,
                &request.url,
                &request.verb,
                request.verb_source,
                None,
            )
        })
        .collect()
}

//! plumber route annotations: `#* @get /path` comment blocks above a handler.

use tree_sitter::Node;

pub(crate) const PLUMBER_VERBS: &[&str] =
    &["get", "post", "put", "delete", "head", "patch", "options"];

/// The `(VERB, path)` routes a `#*` / `#'` annotation block directly above
/// `handler` declares.
pub(crate) fn annotated_routes(content: &str, handler: Node) -> Vec<(String, String)> {
    let mut routes = Vec::new();
    let mut next_row = handler.start_position().row;
    let mut current = handler.prev_sibling();
    while let Some(comment) = current.filter(|node| node.kind() == "comment") {
        if comment.end_position().row + 1 < next_row {
            break;
        }
        let text = content.get(comment.byte_range()).unwrap_or_default();
        if let Some(route) = route_annotation(text) {
            routes.push(route);
        }
        next_row = comment.start_position().row;
        current = comment.prev_sibling();
    }
    routes.reverse();
    routes
}

fn route_annotation(comment: &str) -> Option<(String, String)> {
    let rest = comment
        .strip_prefix("#*")
        .or_else(|| comment.strip_prefix("#'"))?
        .trim_start();
    let rest = rest.strip_prefix('@')?;
    let (verb, path) = rest.split_once(char::is_whitespace)?;
    let path = path.split_whitespace().next()?;
    (PLUMBER_VERBS.contains(&verb) && path.starts_with('/'))
        .then(|| (verb.to_ascii_uppercase(), path.to_string()))
}

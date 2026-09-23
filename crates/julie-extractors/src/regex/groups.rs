/// Check if a group is a capturing group: a plain `(...)` or a named group.
pub(crate) fn is_capturing_group(group_text: &str) -> bool {
    match group_text.strip_prefix("(?") {
        None => group_text.starts_with('('),
        Some(rest) => {
            rest.starts_with("P<")
                || (rest.starts_with('<') && !rest.starts_with("<=") && !rest.starts_with("<!"))
        }
    }
}

/// Extract the name from a named group
pub(crate) fn extract_group_name(group_text: &str) -> Option<String> {
    if let Some(start) = group_text.find("(?<")
        && let Some(end) = group_text[start + 3..].find('>')
    {
        let end_idx = start + 3 + end;
        // SAFETY: Check char boundary before slicing to prevent UTF-8 panic
        if group_text.is_char_boundary(start + 3) && group_text.is_char_boundary(end_idx) {
            return Some(group_text[start + 3..end_idx].to_string());
        }
    }
    if let Some(start) = group_text.find("(?P<")
        && let Some(end) = group_text[start + 4..].find('>')
    {
        let end_idx = start + 4 + end;
        // SAFETY: Check char boundary before slicing to prevent UTF-8 panic
        if group_text.is_char_boundary(start + 4) && group_text.is_char_boundary(end_idx) {
            return Some(group_text[start + 4..end_idx].to_string());
        }
    }
    None
}

/// Returns the `group_name` child of a named capturing group.
pub(crate) fn group_name_node(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == "group_name")
}

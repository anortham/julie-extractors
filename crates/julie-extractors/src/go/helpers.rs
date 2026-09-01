use std::collections::{BTreeSet, HashSet};

use tree_sitter::Node;

use crate::base::{AnnotationMarker, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Helper methods for Go-specific utilities and node text extraction
impl super::GoExtractor {
    /// Check if identifier is public (Go visibility rules)
    /// In Go, identifiers starting with uppercase are public
    pub(super) fn is_public(&self, name: &str) -> bool {
        name.chars().next().is_some_and(|c| c.is_uppercase())
    }

    /// Get node text (helper method)
    pub(super) fn get_node_text(&self, node: Node) -> String {
        self.base.get_node_text(&node)
    }

    /// Extract the type string from a type node
    pub(super) fn extract_type_from_node(&self, node: Node) -> String {
        self.extract_type_from_node_at_depth(node, 0)
    }

    fn extract_type_from_node_at_depth(&self, node: Node, depth: u32) -> String {
        if !should_visit_tree_depth(depth) {
            return self.get_node_text(node);
        }

        match node.kind() {
            "type_identifier" | "primitive_type" => self.get_node_text(node),
            "map_type" => {
                let mut parts = Vec::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    parts.push(self.get_node_text(child));
                }
                parts.join("")
            }
            "slice_type" => {
                let child_depth = child_tree_depth(depth);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() != "[" && child.kind() != "]" {
                        return if let Some(child_depth) = child_depth {
                            format!(
                                "[]{}",
                                self.extract_type_from_node_at_depth(child, child_depth)
                            )
                        } else {
                            self.get_node_text(node)
                        };
                    }
                }
                self.get_node_text(node)
            }
            "array_type" => self.get_node_text(node),
            "pointer_type" => {
                let child_depth = child_tree_depth(depth);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() != "*" {
                        return if let Some(child_depth) = child_depth {
                            format!(
                                "*{}",
                                self.extract_type_from_node_at_depth(child, child_depth)
                            )
                        } else {
                            self.get_node_text(node)
                        };
                    }
                }
                self.get_node_text(node)
            }
            "channel_type" => {
                // Handle channel types like <-chan, chan<-, chan
                self.get_node_text(node)
            }
            "interface_type" => {
                // Handle interface{} and other interface types
                self.get_node_text(node)
            }
            "function_type" => {
                // Handle function types like func(int) string
                self.get_node_text(node)
            }
            "qualified_type" => {
                // Handle types like package.TypeName
                self.get_node_text(node)
            }
            "generic_type" => {
                // Handle generic types like Stack[T]
                self.get_node_text(node)
            }
            "type_arguments" => {
                // Handle type arguments like [T, U]
                self.get_node_text(node)
            }
            _ => self.get_node_text(node),
        }
    }

    /// Extract interface body content for union types and methods
    pub(super) fn extract_interface_body(&self, interface_node: Node) -> String {
        let mut body_parts = Vec::new();
        let mut cursor = interface_node.walk();

        for child in interface_node.children(&mut cursor) {
            if child.kind() == "type_elem" {
                body_parts.push(self.get_node_text(child));
            }
        }

        body_parts.join("; ")
    }

    /// Extract the base type name from a receiver parameter: `Type`, `*Type`,
    /// `Type[T]`, and `*Type[T]` all reduce to `Type`.
    pub(super) fn extract_receiver_type_from_param(&self, param_decl: Node) -> String {
        param_decl
            .child_by_field_name("type")
            .and_then(receiver_base_type_node)
            .map(|type_node| self.get_node_text(type_node))
            .unwrap_or_default()
    }

    /// Extract return type from function signatures like "func getName() string"
    pub(super) fn extract_return_type_from_signature(&self, signature: &str) -> Option<String> {
        if let Some(paren_end) = signature.rfind(')') {
            let after_paren = signature[paren_end + 1..].trim();
            if !after_paren.is_empty() && after_paren != "{" {
                return Some(
                    after_paren
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string(),
                );
            }
        }
        None
    }

    /// Extract type from variable signatures like "var name string = value" or "const name string = value"
    pub(super) fn extract_variable_type_from_signature(&self, signature: &str) -> Option<String> {
        if signature.starts_with("var ") || signature.starts_with("const ") {
            let parts: Vec<&str> = signature.split_whitespace().collect();
            if parts.len() >= 3 {
                let potential_type = parts[2];
                if potential_type != "=" {
                    return Some(potential_type.to_string());
                }
            }
        }
        None
    }

    pub(super) fn annotations_from_struct_field_tags(
        &self,
        struct_node: Node,
    ) -> Vec<AnnotationMarker> {
        let mut tag_keys = BTreeSet::new();
        collect_struct_field_tag_keys(struct_node, &mut tag_keys, &|node| self.get_node_text(node));

        if tag_keys.is_empty() {
            return Vec::new();
        }

        vec![AnnotationMarker {
            annotation: "field_tags".to_string(),
            annotation_key: "field_tags".to_string(),
            raw_text: Some(tag_keys.into_iter().collect::<Vec<_>>().join(", ")),
            carrier: None,
        }]
    }

    pub(super) fn annotations_from_field_tag(&self, tag_text: &str) -> Vec<AnnotationMarker> {
        parse_go_struct_tag_pairs(tag_text)
            .into_iter()
            .map(|(key, raw_fragment)| AnnotationMarker {
                annotation: raw_fragment.clone(),
                annotation_key: key,
                raw_text: Some(raw_fragment),
                carrier: None,
            })
            .collect()
    }

    pub(super) fn find_function_doc_comment(&self, node: &Node) -> Option<String> {
        let comments: Vec<String> = self
            .preceding_comment_texts(node.prev_named_sibling())
            .into_iter()
            .filter(|comment| parse_go_compiler_directive(comment).is_none())
            .collect();
        select_go_doc_comment_block(&comments)
    }

    pub(super) fn annotations_from_compiler_directives(
        &self,
        node: &Node,
    ) -> Vec<AnnotationMarker> {
        let comments = self.preceding_comment_texts(node.prev_named_sibling());
        let mut seen = HashSet::new();
        let mut markers = Vec::new();

        for comment in comments.iter().rev() {
            if let Some(marker) = parse_go_compiler_directive(comment)
                && seen.insert(marker.annotation_key.clone())
            {
                markers.push(marker);
            }
        }

        markers
    }

    fn preceding_comment_texts(&self, mut current: Option<Node>) -> Vec<String> {
        let mut comments = Vec::new();

        while let Some(sibling) = current {
            if sibling.kind().contains("comment") {
                comments.push(self.get_node_text(sibling));
                current = sibling.prev_named_sibling();
            } else {
                break;
            }
        }

        comments
    }
}

fn collect_struct_field_tag_keys(
    node: Node,
    keys: &mut BTreeSet<String>,
    get_text: &dyn Fn(Node) -> String,
) {
    collect_struct_field_tag_keys_at_depth(node, keys, get_text, 0);
}

fn collect_struct_field_tag_keys_at_depth(
    node: Node,
    keys: &mut BTreeSet<String>,
    get_text: &dyn Fn(Node) -> String,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "field_declaration"
        && let Some(tag_node) = node.child_by_field_name("tag")
    {
        for (key, _) in parse_go_struct_tag_pairs(&get_text(tag_node)) {
            keys.insert(key);
        }
        return;
    }

    if matches!(
        node.kind(),
        "tag" | "field_tag" | "raw_string_literal" | "interpreted_string_literal"
    ) {
        for (key, _) in parse_go_struct_tag_pairs(&get_text(node)) {
            keys.insert(key);
        }
        return;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_struct_field_tag_keys_at_depth(child, keys, get_text, child_depth);
    }
}

fn parse_go_struct_tag_pairs(tag_text: &str) -> Vec<(String, String)> {
    let inner = tag_text.trim().trim_matches('`');
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in inner.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            current.push(ch);
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            current.push(ch);
            continue;
        }
        if ch.is_whitespace() && !in_string {
            if !current.is_empty() {
                parts.push(current.clone());
                current.clear();
            }
            continue;
        }
        current.push(ch);
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
        .into_iter()
        .filter_map(|part| {
            let (key, _) = part.split_once(':')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            Some((key.to_string(), part))
        })
        .collect()
}

pub(super) fn finalize_function_symbol(mut symbol: Symbol, doc_comment: Option<String>) -> Symbol {
    if doc_comment.is_none() && !symbol.annotations.is_empty() {
        symbol.doc_comment = None;
    }
    symbol
}

fn receiver_base_type_node(type_node: Node) -> Option<Node> {
    match type_node.kind() {
        "type_identifier" => Some(type_node),
        "pointer_type" => receiver_base_type_node(type_node.named_child(0)?),
        "generic_type" => receiver_base_type_node(type_node.child_by_field_name("type")?),
        _ => None,
    }
}

fn select_go_doc_comment_block(comments_nearest_first: &[String]) -> Option<String> {
    let spec = crate::language::language_spec("go")?;
    if comments_nearest_first.is_empty() {
        return None;
    }

    let comments_top_down: Vec<_> = comments_nearest_first.iter().rev().collect();
    for start_index in 0..comments_top_down.len() {
        let first = comments_top_down[start_index];
        if !spec.is_doc_comment(first) {
            continue;
        }

        if comments_top_down[start_index + 1..]
            .iter()
            .all(|comment| spec.continues_doc_comment(comment))
        {
            return Some(
                comments_top_down[start_index..]
                    .iter()
                    .map(|comment| comment.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }

    None
}

fn parse_go_compiler_directive(comment: &str) -> Option<AnnotationMarker> {
    let trimmed = comment.trim();
    let body = trimmed.strip_prefix("//")?.trim_start();
    let directive = body.strip_prefix("go:")?.trim_start();
    let name = directive
        .split(|ch: char| ch.is_whitespace() || ch == '(')
        .next()?
        .trim();
    if name.is_empty() {
        return None;
    }

    Some(AnnotationMarker {
        annotation: format!("go:{name}"),
        annotation_key: name.to_ascii_lowercase(),
        raw_text: Some(trimmed.to_string()),
        carrier: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_go_compiler_directive, parse_go_struct_tag_pairs};

    #[test]
    fn parse_struct_tag_pairs_splits_multiple_keys() {
        let pairs = parse_go_struct_tag_pairs(r#"`json:"id" db:"worker_id"`"#);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, "json");
        assert_eq!(pairs[0].1, r#"json:"id""#);
        assert_eq!(pairs[1].0, "db");
        assert_eq!(pairs[1].1, r#"db:"worker_id""#);
    }

    #[test]
    fn parse_compiler_directive_extracts_go_prefix() {
        let marker = parse_go_compiler_directive("//go:noinline").expect("directive");
        assert_eq!(marker.annotation_key, "noinline");
        assert_eq!(marker.annotation, "go:noinline");
        assert_eq!(marker.raw_text.as_deref(), Some("//go:noinline"));
    }
}

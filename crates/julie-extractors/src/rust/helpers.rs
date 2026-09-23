/// Helper utilities for Rust extractor
/// - Impl block tracking
/// - Visibility and attribute extraction
/// - Keyword detection
use crate::base::{BaseExtractor, Visibility, normalize_annotations};
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node;

// Static regex compiled once for performance
static DOC_ATTRIBUTE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"#\[doc\s*=\s*"([^"]+)"\]"#).unwrap());

/// Information about an impl block (stored by byte range for safety)
#[derive(Debug, Clone)]
pub struct ImplBlockInfo {
    /// Byte range of the impl block in the source file (safe to store)
    pub start_byte: usize,
    pub end_byte: usize,
    pub type_name: String,
    /// The symbol that encloses the impl block (a module or function), used to
    /// pick the implemented type among same-named types in sibling scopes.
    pub parent_id: Option<String>,
    /// Index into the extractor's re-parsed macro trees when the impl block was
    /// written inside an item macro; `None` for the file's own tree.
    pub tree_index: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImplTargetNames {
    pub trait_name: Option<String>,
    pub type_name: Option<String>,
}

pub(super) fn extract_impl_target_names(base: &BaseExtractor, node: Node) -> ImplTargetNames {
    let name_of = |field: &str| {
        node.child_by_field_name(field)
            .and_then(super::type_facts::base_type_name_node)
            .map(|name| base.get_node_text(&name))
    };
    ImplTargetNames {
        trait_name: name_of("trait"),
        type_name: name_of("type"),
    }
}

/// Collect the segments of a path expression, dropping turbofish type arguments.
pub(super) fn push_path_segments(base: &BaseExtractor, node: Node, segments: &mut Vec<String>) {
    match node.kind() {
        "scoped_identifier" | "scoped_type_identifier" => {
            if let Some(path) = node.child_by_field_name("path") {
                push_path_segments(base, path, segments);
            }
            if let Some(name) = node.child_by_field_name("name") {
                segments.push(base.get_node_text(&name));
            }
        }
        "generic_type_with_turbofish" | "generic_type" => {
            if let Some(inner) = node.child_by_field_name("type") {
                push_path_segments(base, inner, segments);
            }
        }
        _ => segments.push(base.get_node_text(&node)),
    }
}

/// One name bound by a `use` declaration or `extern crate`.
pub(super) struct UseLeaf {
    pub name: String,
    pub path: Vec<String>,
    pub alias: Option<String>,
}

/// Flatten a `use` tree (or `extern crate`) into the names it binds. A glob
/// binds its prefix path, named `a::b` for `use a::b::*`.
pub(super) fn use_leaves(base: &BaseExtractor, node: Node) -> Vec<UseLeaf> {
    let mut leaves = Vec::new();
    match node.kind() {
        "use_declaration" => {
            if let Some(argument) = node.child_by_field_name("argument") {
                collect_use_leaves(base, argument, &[], &mut leaves);
            }
        }
        "extern_crate_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                let crate_name = base.get_node_text(&name);
                let alias = node
                    .child_by_field_name("alias")
                    .map(|alias| base.get_node_text(&alias));
                leaves.push(UseLeaf {
                    name: alias.clone().unwrap_or_else(|| crate_name.clone()),
                    path: vec![crate_name],
                    alias,
                });
            }
        }
        _ => {}
    }
    leaves
}

fn collect_use_leaves(
    base: &BaseExtractor,
    node: Node,
    prefix: &[String],
    leaves: &mut Vec<UseLeaf>,
) {
    collect_use_leaves_at(base, node, prefix, leaves, 0);
}

fn collect_use_leaves_at(
    base: &BaseExtractor,
    node: Node,
    prefix: &[String],
    leaves: &mut Vec<UseLeaf>,
    depth: u32,
) {
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return;
    };
    let joined = |tail: Option<Node>| {
        let mut path = prefix.to_vec();
        if let Some(tail) = tail {
            push_path_segments(base, tail, &mut path);
        }
        path
    };
    match node.kind() {
        "use_as_clause" => {
            let path = joined(node.child_by_field_name("path"));
            let alias = node
                .child_by_field_name("alias")
                .map(|alias| base.get_node_text(&alias));
            if let Some(alias) = alias {
                leaves.push(UseLeaf {
                    name: alias.clone(),
                    path,
                    alias: Some(alias),
                });
            }
        }
        "scoped_use_list" => {
            let path = joined(node.child_by_field_name("path"));
            if let Some(list) = node.child_by_field_name("list") {
                collect_use_leaves_at(base, list, &path, leaves, child_depth);
            }
        }
        "use_list" => {
            for item in node.named_children(&mut node.walk()) {
                collect_use_leaves_at(base, item, prefix, leaves, child_depth);
            }
        }
        "use_wildcard" => {
            let path = joined(node.named_child(0));
            if !path.is_empty() {
                leaves.push(UseLeaf {
                    name: path.join("::"),
                    path,
                    alias: None,
                });
            }
        }
        "self" if !prefix.is_empty() => leaves.push(UseLeaf {
            name: prefix[prefix.len() - 1].clone(),
            path: prefix.to_vec(),
            alias: None,
        }),
        "line_comment" | "block_comment" => {}
        _ => {
            let path = joined(Some(node));
            if let Some(name) = path.last().cloned() {
                leaves.push(UseLeaf {
                    name,
                    path,
                    alias: None,
                });
            }
        }
    }
}

/// A call site written inside a macro invocation's token tree. Tree-sitter
/// leaves macro arguments as flat tokens, so `name(` / `recv.name(` /
/// `a::b::name(` are recognized from the token sequence.
pub(super) struct MacroTokenCall {
    pub name: String,
    pub receiver: Option<String>,
    pub namespace_path: Vec<String>,
}

pub(super) fn macro_token_call(base: &BaseExtractor, node: Node) -> Option<MacroTokenCall> {
    if node.kind() != "identifier" || !token_tree_belongs_to_macro_invocation(node) {
        return None;
    }
    let arguments = node.next_sibling()?;
    if arguments.kind() != "token_tree" || arguments.child(0)?.kind() != "(" {
        return None;
    }
    let mut call = MacroTokenCall {
        name: base.get_node_text(&node),
        receiver: None,
        namespace_path: Vec::new(),
    };
    let Some(previous) = node.prev_sibling() else {
        return Some(call);
    };
    match previous.kind() {
        "." => {
            let mut start = previous;
            while let Some(token) = start.prev_sibling() {
                if !matches!(
                    token.kind(),
                    "identifier" | "self" | "token_tree" | "." | "::" | "?" | "integer_literal"
                ) {
                    break;
                }
                start = token;
            }
            if start.id() != previous.id() {
                call.receiver = base
                    .content
                    .get(start.start_byte()..previous.start_byte())
                    .map(str::to_owned);
            }
        }
        "::" => {
            let mut separator = previous;
            while let Some(segment) = separator.prev_sibling() {
                if !matches!(segment.kind(), "identifier" | "crate" | "self" | "super") {
                    break;
                }
                call.namespace_path.insert(0, base.get_node_text(&segment));
                match segment.prev_sibling() {
                    Some(next) if next.kind() == "::" => separator = next,
                    _ => break,
                }
            }
        }
        "fn" | "macro_rules!" => return None,
        _ => {}
    }
    Some(call)
}

/// True when a `token_tree` interior node belongs to a macro INVOCATION (its
/// tokens are call-site expressions), as opposed to a `macro_rules!` body or an
/// attribute argument list.
pub(super) fn token_tree_belongs_to_macro_invocation(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "token_tree" => current = parent,
            "macro_invocation" => return true,
            _ => return false,
        }
    }
    false
}

/// Extract visibility modifier from a node (pub, pub(crate), etc.)
pub(super) fn extract_visibility(base: &BaseExtractor, node: Node) -> String {
    let visibility_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "visibility_modifier");

    if let Some(vis_node) = visibility_node {
        let vis_text = base.get_node_text(&vis_node);
        if vis_text == "pub" {
            "pub ".to_string()
        } else if vis_text.starts_with("pub(") {
            format!("{} ", vis_text)
        } else {
            String::new()
        }
    } else {
        String::new()
    }
}

/// The visibility a node's own modifier states: `pub` is public, the
/// restricted forms (`pub(crate)`, `pub(super)`, `pub(in path)`) are internal,
/// and `pub(self)` or no modifier is private.
fn declared_visibility(base: &BaseExtractor, node: Node) -> Visibility {
    let Some(modifier) = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "visibility_modifier")
    else {
        return Visibility::Private;
    };
    let text: String = base
        .get_node_text(&modifier)
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    match text.as_str() {
        "pub" => Visibility::Public,
        "pub(self)" => Visibility::Private,
        _ => Visibility::Internal,
    }
}

/// The visibility a declaration actually has. Trait members take the trait's
/// visibility, trait-impl members are as public as the trait they implement,
/// and enum variants and their fields take the enum's visibility.
pub(super) fn effective_visibility(base: &BaseExtractor, node: Node) -> Visibility {
    if let Some(owner) = associated_item_owner(node) {
        if owner.kind() == "trait_item" {
            return declared_visibility(base, owner);
        }
        if owner.child_by_field_name("trait").is_some() {
            return Visibility::Public;
        }
    }
    if let Some(enum_item) = enclosing_enum_of_member(node) {
        return declared_visibility(base, enum_item);
    }
    declared_visibility(base, node)
}

/// The `enum_item` whose variant (or variant field) this node is.
fn enclosing_enum_of_member(node: Node) -> Option<Node> {
    let variant = match node.kind() {
        "enum_variant" => node,
        "field_declaration" => node
            .parent()
            .and_then(|list| list.parent())
            .filter(|owner| owner.kind() == "enum_variant")?,
        _ => return None,
    };
    variant
        .parent()
        .and_then(|list| list.parent())
        .filter(|owner| owner.kind() == "enum_item")
}

/// The `impl_item` or `trait_item` a declaration is directly associated with.
pub(super) fn associated_item_owner(node: Node) -> Option<Node> {
    node.parent()
        .filter(|list| list.kind() == "declaration_list")
        .and_then(|list| list.parent())
        .filter(|owner| matches!(owner.kind(), "impl_item" | "trait_item"))
}

/// Attributes directly above a node (like `#[derive(...)]`), in source order.
/// Comments between the attributes and the item are passed over.
pub(super) fn get_preceding_attributes<'a>(_base: &BaseExtractor, node: Node<'a>) -> Vec<Node<'a>> {
    let mut attributes = Vec::new();
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            "attribute_item" => attributes.push(sibling),
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        previous = sibling.prev_sibling();
    }
    attributes.reverse();
    attributes
}

/// Normalized annotation rows for the attributes directly above a node.
pub(super) fn item_annotations(
    base: &BaseExtractor,
    node: Node,
) -> Vec<crate::base::AnnotationMarker> {
    let attributes = get_preceding_attributes(base, node);
    normalize_annotations(&extract_attribute_texts(base, &attributes), "rust")
}

/// Extract raw attribute text from attribute nodes.
pub(super) fn extract_attribute_texts(base: &BaseExtractor, attributes: &[Node]) -> Vec<String> {
    attributes
        .iter()
        .map(|attribute| base.get_node_text(attribute))
        .collect()
}

/// Whether any attribute is a `cfg` whose predicate selects test builds.
///
/// Accepts the bare `#[cfg(test)]` and the compound `#[cfg(all(test, ..))]` and
/// `#[cfg(any(test, ..))]` forms, at any nesting depth. A `test` inside a `not`
/// means the item is compiled out of test builds, so a `not` subtree never
/// contributes.
pub(super) fn has_cfg_test_attribute(base: &BaseExtractor, attributes: &[Node<'_>]) -> bool {
    attributes
        .iter()
        .copied()
        .any(|attribute_item| is_cfg_test_attribute(base, attribute_item))
}

fn is_cfg_test_attribute(base: &BaseExtractor, attribute_item: Node<'_>) -> bool {
    let Some(attribute) = attribute_item.named_child(0) else {
        return false;
    };
    if attribute.kind() != "attribute" {
        return false;
    }

    let Some(name) = attribute
        .named_children(&mut attribute.walk())
        .find(|child| child.kind() == "identifier")
    else {
        return false;
    };
    if base.get_node_text(&name) != "cfg" {
        return false;
    }

    let Some(arguments) = attribute.child_by_field_name("arguments") else {
        return false;
    };
    cfg_predicate_selects_test(base, arguments)
}

/// Iterative on purpose: a nested `cfg` predicate is CST recursion, and the
/// crate-wide traversal budget exists because one Rust frame per CST node
/// overflows the extraction worker's stack on a generated file.
fn cfg_predicate_selects_test<'tree>(base: &BaseExtractor, predicate: Node<'tree>) -> bool {
    let mut pending = vec![predicate];

    while let Some(node) = pending.pop() {
        let terms: Vec<_> = node.named_children(&mut node.walk()).collect();
        let mut index = 0;
        while index < terms.len() {
            let term = terms[index];
            if term.kind() != "identifier" {
                index += 1;
                continue;
            }

            let name = base.get_node_text(&term);
            if name == "test" {
                return true;
            }

            let nested = terms
                .get(index + 1)
                .copied()
                .filter(|child| child.kind() == "token_tree");
            match (name.as_str(), nested) {
                ("all" | "any", Some(inner)) => {
                    pending.push(inner);
                    index += 2;
                }
                ("not", Some(_)) => index += 2,
                _ => index += 1,
            }
        }
    }

    false
}

/// Extract trait names from #[derive(...)] attributes
pub(super) fn extract_derived_traits(base: &BaseExtractor, attributes: &[Node]) -> Vec<String> {
    let mut traits = Vec::new();

    for attr in attributes {
        // Look for derive attribute
        let attribute_node = attr
            .children(&mut attr.walk())
            .find(|c| c.kind() == "attribute");

        if let Some(attr_node) = attribute_node {
            let identifier_node = attr_node
                .children(&mut attr_node.walk())
                .find(|c| c.kind() == "identifier");

            if let Some(ident) = identifier_node
                && base.get_node_text(&ident) == "derive"
            {
                // Find the token tree with the trait list
                let token_tree = attr_node
                    .children(&mut attr_node.walk())
                    .find(|c| c.kind() == "token_tree");

                if let Some(tree) = token_tree {
                    for child in tree.children(&mut tree.walk()) {
                        if child.kind() == "identifier" {
                            traits.push(base.get_node_text(&child));
                        }
                    }
                }
            }
        }
    }

    traits
}

/// Check if node has async keyword
pub(super) fn has_async_keyword(base: &BaseExtractor, node: Node) -> bool {
    node.children(&mut node.walk())
        .any(|c| c.kind() == "async" || base.get_node_text(&c) == "async")
}

/// Check if node has unsafe keyword
pub(super) fn has_unsafe_keyword(base: &BaseExtractor, node: Node) -> bool {
    node.children(&mut node.walk())
        .any(|c| c.kind() == "unsafe" || base.get_node_text(&c) == "unsafe")
}

/// Extract extern modifier from a function node
pub(super) fn extract_extern_modifier(base: &BaseExtractor, node: Node) -> String {
    let function_modifiers_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "function_modifiers");

    if let Some(modifiers) = function_modifiers_node {
        let extern_modifier_node = modifiers
            .children(&mut modifiers.walk())
            .find(|c| c.kind() == "extern_modifier");

        if let Some(extern_node) = extern_modifier_node {
            return base.get_node_text(&extern_node);
        }
    }

    String::new()
}

/// The rustdoc text for a node: the outer doc comments (`///`, `/** */`)
/// directly above it, passing over attributes and plain comments, or its inner
/// doc comments (`//!`, `/*! */`) when it is a module with a body, or a
/// `#[doc = "..."]` attribute. Inner doc comments above a node document the
/// enclosing item, never the node.
pub(super) fn find_doc_comment(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut doc_blocks = Vec::new();
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            "attribute_item" | "attribute" => {}
            "line_comment" | "block_comment" => match classify_comment(base, sibling) {
                CommentDoc::Outer(text) => doc_blocks.push(text),
                CommentDoc::Inner => break,
                CommentDoc::Plain => {}
            },
            _ => break,
        }
        previous = sibling.prev_sibling();
    }
    if !doc_blocks.is_empty() {
        doc_blocks.reverse();
        let doc = doc_blocks
            .into_iter()
            .filter(|block| !block.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !doc.is_empty() {
            return Some(doc);
        }
    }

    if let Some(doc_comment) = find_inner_doc_comment(base, node) {
        return Some(doc_comment);
    }

    get_preceding_attributes(base, node)
        .into_iter()
        .find_map(|attribute| extract_doc_from_attribute(base, attribute))
}

enum CommentDoc {
    Outer(String),
    Inner,
    Plain,
}

fn classify_comment(base: &BaseExtractor, comment: Node) -> CommentDoc {
    let text = base.get_node_text(&comment);
    let text = text.trim();
    if let Some(body) = text.strip_prefix("///") {
        if body.starts_with('/') {
            return CommentDoc::Plain;
        }
        return CommentDoc::Outer(body.trim().to_string());
    }
    if text.starts_with("//!") || text.starts_with("/*!") {
        return CommentDoc::Inner;
    }
    if let Some(body) = text.strip_prefix("/**")
        && !body.starts_with('*')
        && body != "/"
    {
        return CommentDoc::Outer(strip_block_comment_body(body));
    }
    CommentDoc::Plain
}

fn strip_block_comment_body(body: &str) -> String {
    body.strip_suffix("*/")
        .unwrap_or(body)
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_inner_doc_comment(base: &BaseExtractor, node: Node) -> Option<String> {
    let body = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "declaration_list")?;

    let mut doc_comments = Vec::new();
    for child in body.children(&mut body.walk()) {
        match child.kind() {
            "{" | "}" => continue,
            "line_comment" => {
                let comment_text = base.get_node_text(&child);
                let comment_text = comment_text.trim_start();
                if let Some(stripped) = comment_text.strip_prefix("//!") {
                    let doc_text = stripped.trim();
                    if !doc_text.is_empty() {
                        doc_comments.push(doc_text.to_string());
                    }
                    continue;
                }
                break;
            }
            "block_comment" => {
                let comment_text = base.get_node_text(&child);
                let comment_text = comment_text.trim_start();
                if let Some(stripped) = comment_text.strip_prefix("/*!") {
                    let doc_text = strip_block_comment_body(stripped);
                    if !doc_text.is_empty() {
                        doc_comments.push(doc_text);
                    }
                    continue;
                }
                break;
            }
            _ => break,
        }
    }

    if doc_comments.is_empty() {
        None
    } else {
        Some(doc_comments.join("\n"))
    }
}

/// Extract doc string from #[doc = "..."] attribute
pub(super) fn extract_doc_from_attribute(base: &BaseExtractor, node: Node) -> Option<String> {
    let attr_text = base.get_node_text(&node);
    if let Some(captures) = DOC_ATTRIBUTE_RE.captures(&attr_text)
        && let Some(doc_match) = captures.get(1)
    {
        return Some(doc_match.as_str().to_string());
    }
    None
}

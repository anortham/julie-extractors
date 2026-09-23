//! Readings of Markdown block nodes shared by symbols, facts, and source
//! regions: heading names and explicit anchors, fenced-code languages,
//! frontmatter keys, embedded bodies, and links inside HTML blocks.

use std::sync::OnceLock;

use regex::Regex;
use tree_sitter::Node;

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// The heading text without markup, closing `#` sequence, or `{#id}`
/// attribute block, and the explicit id when there is one. An empty heading
/// has no name.
pub(crate) fn heading_name(content: &str, heading: Node<'_>) -> Option<(String, Option<String>)> {
    let text = match find_descendant(heading, "inline", 0) {
        Some(inline) => super::inline::parse_inline_node(inline, content)
            .map(|tree| super::inline::plain_text(content, tree.root_node()))
            .unwrap_or_else(|| collapse(content.get(inline.byte_range()).unwrap_or(""))),
        None => String::new(),
    };
    let (text, anchor) = split_attribute_block(&text);
    let name = strip_closing_hashes(&text);
    (!name.is_empty()).then_some((name, anchor))
}

fn find_descendant<'tree>(node: Node<'tree>, kind: &str, depth: u32) -> Option<Node<'tree>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().find_map(|child| {
        if child.kind() == kind {
            Some(child)
        } else {
            find_descendant(child, kind, child_depth)
        }
    })
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `Configuration {#custom-id .class}` -> (`Configuration`, `custom-id`).
fn split_attribute_block(text: &str) -> (String, Option<String>) {
    static ATTRIBUTES: OnceLock<Regex> = OnceLock::new();
    let attributes = ATTRIBUTES.get_or_init(|| {
        Regex::new(r"\s*\{\s*((?:[#.][^\s{}]+|[A-Za-z_][\w-]*=\S+)(?:\s+(?:[#.][^\s{}]+|[A-Za-z_][\w-]*=\S+))*)\s*\}\s*$")
            .expect("attribute block pattern")
    });
    let Some(captures) = attributes.captures(text) else {
        return (text.to_string(), None);
    };
    let anchor = captures[1]
        .split_whitespace()
        .find_map(|part| part.strip_prefix('#'))
        .map(str::to_string);
    let whole = captures
        .get(0)
        .map_or(text.len(), |matched| matched.start());
    (text[..whole].to_string(), anchor)
}

/// The optional closing sequence of an ATX heading: `#`s preceded by a space
/// (or making up the whole heading).
fn strip_closing_hashes(text: &str) -> String {
    let trimmed = text.trim_end();
    let without = trimmed.trim_end_matches('#');
    if without.len() == trimmed.len() {
        return trimmed.trim().to_string();
    }
    if without.is_empty() || without.ends_with([' ', '\t']) {
        return without.trim().to_string();
    }
    trimmed.trim().to_string()
}

/// The language of a fenced code block: the grammar's `language` token of
/// the info string with Pandoc braces and dots and rustdoc attributes
/// removed (`rust,no_run` -> `rust`, `{.python}` -> `python`, `{r echo=FALSE}` -> `r`).
pub(crate) fn fence_language(content: &str, fence: Node<'_>) -> Option<String> {
    let mut cursor = fence.walk();
    let info = fence
        .children(&mut cursor)
        .find(|child| child.kind() == "info_string")?;
    let mut info_cursor = info.walk();
    let token = info
        .children(&mut info_cursor)
        .find(|child| child.kind() == "language")
        .and_then(|language| content.get(language.byte_range()))
        .or_else(|| content.get(info.byte_range())?.split_whitespace().next())?;
    normalize_fence_language(token)
}

fn normalize_fence_language(token: &str) -> Option<String> {
    let token = token.trim().trim_start_matches(['{', '.']);
    let end = token
        .find(|ch: char| matches!(ch, ',' | '}' | '=' | '{') || ch.is_whitespace())
        .unwrap_or(token.len());
    let language = token[..end].trim_start_matches('.');
    (!language.is_empty()).then(|| language.to_string())
}

/// The byte range between a frontmatter block's delimiter lines.
pub(crate) fn frontmatter_body(content: &str, node: Node<'_>) -> Option<(usize, usize)> {
    let text = content.get(node.byte_range())?;
    let first_break = text.find('\n')? + 1;
    let trimmed = text.trim_end();
    let last_line = trimmed.rfind('\n').map_or(0, |index| index + 1);
    let closing = trimmed[last_line..].trim();
    let body_end = if matches!(closing, "---" | "+++" | "...") {
        last_line
    } else {
        trimmed.len()
    };
    (body_end > first_break).then(|| {
        (
            node.start_byte() + first_break,
            node.start_byte() + body_end,
        )
    })
}

/// A top-level frontmatter key: its name, the raw value on its line, and the
/// byte range from the key through its last continuation line.
pub(crate) struct FrontmatterKey {
    pub name: String,
    pub value: Option<String>,
    pub start: usize,
    pub end: usize,
}

/// Top-level keys of a YAML (`---`) or TOML (`+++`) frontmatter block. TOML
/// table headers count as keys named by the table.
pub(crate) fn frontmatter_keys(content: &str, node: Node<'_>) -> Vec<FrontmatterKey> {
    let Some((start, end)) = frontmatter_body(content, node) else {
        return Vec::new();
    };
    let toml = node.kind() == "plus_metadata";
    let mut keys: Vec<FrontmatterKey> = Vec::new();
    let mut offset = start;
    let mut in_table = false;
    for line in content[start..end].split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let text = line.trim_end();
        let line_end = line_start + text.len();
        if text.trim().is_empty() || text.trim_start().starts_with('#') {
            continue;
        }
        let table_header = toml && text.trim_start().starts_with('[');
        let top_level = !text.starts_with([' ', '\t']) && (table_header || !(toml && in_table));
        let entry = if !top_level {
            None
        } else if toml {
            toml_key(text)
        } else {
            yaml_key(text)
        };
        if table_header {
            in_table = entry.is_some();
        }
        match entry {
            Some((name, value)) => keys.push(FrontmatterKey {
                name,
                value,
                start: line_start,
                end: line_end,
            }),
            None => {
                if let Some(last) = keys.last_mut()
                    && (!top_level || text.starts_with('-') || in_table)
                {
                    last.end = line_end;
                }
            }
        }
    }
    keys
}

fn yaml_key(line: &str) -> Option<(String, Option<String>)> {
    if line.starts_with(['-', '#']) {
        return None;
    }
    let (key, value) = line.split_once(':')?;
    let key = key.trim().trim_matches(['"', '\'']);
    if key.is_empty() || key.contains(char::is_whitespace) && !line.starts_with(['"', '\'']) {
        return None;
    }
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|inner| inner.strip_suffix('\''))
        })
        .unwrap_or(value);
    Some((
        key.to_string(),
        (!value.is_empty()).then(|| value.to_string()),
    ))
}

fn toml_key(line: &str) -> Option<(String, Option<String>)> {
    let trimmed = line.trim();
    if let Some(table) = trimmed
        .strip_prefix('[')
        .map(|rest| rest.trim_start_matches('['))
        .and_then(|rest| rest.split(']').next())
    {
        let table = table.trim();
        return (!table.is_empty()).then(|| (crate::toml::text::decode_toml_key(table), None));
    }
    let (key, value) = trimmed.split_once('=')?;
    let key = crate::toml::text::decode_toml_key(key.trim());
    let value = value.trim();
    let value = crate::toml::text::decode_toml_string(value)
        .map_or_else(|| value.to_string(), |(text, _)| text);
    (!key.is_empty()).then(|| (key, (!value.is_empty()).then_some(value)))
}

/// The language and byte range of a block whose body is another language:
/// frontmatter (`yaml` or `toml`, delimiters excluded) and an HTML block
/// that is not a comment.
pub(crate) fn embedded_body(content: &str, node: Node<'_>) -> Option<(&'static str, usize, usize)> {
    match node.kind() {
        "minus_metadata" => {
            frontmatter_body(content, node).map(|(start, end)| ("yaml", start, end))
        }
        "plus_metadata" => frontmatter_body(content, node).map(|(start, end)| ("toml", start, end)),
        "html_block" => {
            let text = content.get(node.byte_range())?;
            let body = text.trim_end();
            (!body.trim_start().starts_with("<!--") && !body.trim().is_empty())
                .then(|| ("html", node.start_byte(), node.start_byte() + body.len()))
        }
        _ => None,
    }
}

/// An `<a href>` or `<img src>` inside an HTML block.
pub(crate) struct HtmlLink {
    pub image: bool,
    pub start: usize,
    pub end: usize,
    pub destination: String,
    pub destination_start: usize,
    pub text: String,
}

pub(crate) fn html_links(content: &str, node: Node<'_>) -> Vec<HtmlLink> {
    static ANCHOR: OnceLock<Regex> = OnceLock::new();
    static IMAGE: OnceLock<Regex> = OnceLock::new();
    static ALT: OnceLock<Regex> = OnceLock::new();
    let anchor = ANCHOR.get_or_init(|| {
        Regex::new(r#"(?is)<a\b[^>]*?\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')[^>]*>(.*?)</a\s*>"#)
            .expect("anchor pattern")
    });
    let image = IMAGE.get_or_init(|| {
        Regex::new(r#"(?is)<img\b[^>]*?\bsrc\s*=\s*(?:"([^"]*)"|'([^']*)')[^>]*>"#)
            .expect("image pattern")
    });
    let alt = ALT.get_or_init(|| {
        Regex::new(r#"(?is)\balt\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("alt pattern")
    });
    let Some(text) = content.get(node.byte_range()) else {
        return Vec::new();
    };
    if text.trim_start().starts_with("<!--") {
        return Vec::new();
    }
    let base = node.start_byte();
    let mut links = Vec::new();
    for (pattern, is_image) in [(anchor, false), (image, true)] {
        for captures in pattern.captures_iter(text) {
            let Some(whole) = captures.get(0) else {
                continue;
            };
            let Some(destination) = captures.get(1).or_else(|| captures.get(2)) else {
                continue;
            };
            if destination.as_str().trim().is_empty() {
                continue;
            }
            let label = if is_image {
                alt.captures(whole.as_str())
                    .and_then(|alt| alt.get(1).or_else(|| alt.get(2)))
                    .map(|alt| alt.as_str().to_string())
                    .unwrap_or_default()
            } else {
                strip_tags(captures.get(3).map_or("", |inner| inner.as_str()))
            };
            links.push(HtmlLink {
                image: is_image,
                start: base + whole.start(),
                end: base + whole.end(),
                destination: destination.as_str().trim().to_string(),
                destination_start: base + destination.start(),
                text: collapse(&label),
            });
        }
    }
    links.sort_by_key(|link| link.start);
    links
}

fn strip_tags(html: &str) -> String {
    static TAG: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r"<[^>]*>").expect("tag pattern"));
    tag.replace_all(html, " ").into_owned()
}

/// A definition-list entry: the term, the definition, and the byte range of
/// the `: definition` line.
pub(crate) struct DefinitionItem {
    pub term: String,
    pub definition: String,
    pub start: usize,
    pub end: usize,
}

/// Paragraph lines of the form `Term` then `: Definition` (Pandoc and PHP
/// Markdown Extra definition lists, which CommonMark reads as a paragraph).
pub(crate) fn definition_items(content: &str, paragraph: Node<'_>) -> Vec<DefinitionItem> {
    let Some(text) = content.get(paragraph.byte_range()) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    let mut term: Option<&str> = None;
    let mut offset = paragraph.start_byte();
    for raw in text.split_inclusive('\n') {
        let line_start = offset;
        offset += raw.len();
        let line = raw.trim_end();
        match line.strip_prefix(": ").or_else(|| line.strip_prefix(":\t")) {
            Some(definition) => {
                if let Some(term) = term {
                    items.push(DefinitionItem {
                        term: term.trim().to_string(),
                        definition: definition.trim().to_string(),
                        start: line_start,
                        end: line_start + line.len(),
                    });
                }
            }
            None if !line.trim().is_empty() => term = Some(line),
            None => {}
        }
    }
    items
}

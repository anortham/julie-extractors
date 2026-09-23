//! `go.mod` directives read from the tree once and shared by the extractor and
//! the structural-fact collector.
//!
//! Each [`Entry`] is one directive line: a single-line directive, or one line
//! of a `( ... )` block. Its span runs from the directive keyword (single-line
//! form) or the first value (block form) to the end of the last value, so the
//! trailing `// indirect` comment and the line break stay outside it.
//!
//! Comments follow `golang.org/x/mod/modfile`: the `//` lines directly above a
//! line are its leading comments, a `//` comment after the values on the same
//! line is its suffix comment, and a blank line detaches a comment block.

use tree_sitter::Node;

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Directive {
    Module,
    Go,
    Toolchain,
    Require,
    Exclude,
    Replace,
    Retract,
    Tool,
    Ignore,
    Godebug,
}

impl Directive {
    pub(crate) fn keyword(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Go => "go",
            Self::Toolchain => "toolchain",
            Self::Require => "require",
            Self::Exclude => "exclude",
            Self::Replace => "replace",
            Self::Retract => "retract",
            Self::Tool => "tool",
            Self::Ignore => "ignore",
            Self::Godebug => "godebug",
        }
    }

    fn for_node_kind(kind: &str) -> Option<Self> {
        Some(match kind {
            "module_directive" => Self::Module,
            "go_directive" => Self::Go,
            "toolchain_directive" => Self::Toolchain,
            "require_directive" => Self::Require,
            "exclude_directive" => Self::Exclude,
            "replace_directive" => Self::Replace,
            "retract_directive" => Self::Retract,
            "tool_directive" => Self::Tool,
            "ignore_directive" => Self::Ignore,
            "godebug_directive" => Self::Godebug,
            _ => return None,
        })
    }
}

/// One value of a directive line, unquoted, with its role in the directive:
/// `module_path`, `version`, `replacement`, `replacement_version`, `low`,
/// `high`, `toolchain`, `package_path`, `path`, `key`, or `value`.
pub(crate) struct Value<'tree> {
    pub node: Node<'tree>,
    pub text: String,
    pub role: &'static str,
    /// Written as a Go string literal (`"..."` or a raw string).
    pub quoted: bool,
}

pub(crate) struct Entry<'tree> {
    pub directive: Directive,
    pub node: Node<'tree>,
    /// The directive node of a `( ... )` block that holds this line.
    pub block: Option<Node<'tree>>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub values: Vec<Value<'tree>>,
    /// A retract line written as a `[low, high]` interval.
    pub range: bool,
}

impl Entry<'_> {
    pub(crate) fn value(&self, role: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|value| value.role == role)
            .map(|value| value.text.as_str())
    }

    /// The symbol name of a line that declares something: the module path,
    /// a required module, a tool package, the `go` and `toolchain`
    /// settings, or a `godebug` key. Replace, exclude, retract, and ignore
    /// lines are facts only.
    pub(crate) fn symbol_name(&self) -> Option<&str> {
        match self.directive {
            Directive::Module | Directive::Require => self.value("module_path"),
            Directive::Tool => self.value("package_path"),
            Directive::Go | Directive::Toolchain => Some(self.directive.keyword()),
            Directive::Godebug => self.value("key"),
            _ => None,
        }
    }
}

/// Every directive line that parsed cleanly. A line whose node holds a syntax
/// error yields nothing, so a directive the grammar does not know never
/// surfaces as a misread neighbour.
pub(crate) fn entries<'tree>(root: Node<'tree>, content: &str) -> Vec<Entry<'tree>> {
    let mut entries = Vec::new();
    let mut cursor = root.walk();
    for node in root.named_children(&mut cursor) {
        let Some(directive) = Directive::for_node_kind(node.kind()) else {
            continue;
        };
        match directive {
            Directive::Module | Directive::Go | Directive::Toolchain => {
                push_entry(&mut entries, directive, node, None, node, content);
            }
            _ => {
                let block = has_child_kind(node, "(").then_some(node);
                let mut lines = node.walk();
                for line in node
                    .named_children(&mut lines)
                    .filter(|child| child.kind() != "comment")
                {
                    let anchor = if block.is_some() { line } else { node };
                    push_entry(&mut entries, directive, line, block, anchor, content);
                }
            }
        }
    }
    entries
}

fn push_entry<'tree>(
    entries: &mut Vec<Entry<'tree>>,
    directive: Directive,
    node: Node<'tree>,
    block: Option<Node<'tree>>,
    anchor: Node<'tree>,
    content: &str,
) {
    if contains_error(node, 0) {
        return;
    }
    let value_nodes: Vec<Node<'tree>> = if directive == Directive::Tool {
        vec![node]
    } else {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .filter(|child| child.kind() != "comment")
            .collect()
    };
    let arrow = child_start(node, "=>");
    let range = directive == Directive::Retract && has_child_kind(node, "[");
    let values: Vec<Value<'tree>> = value_nodes
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let written = node_text(content, *value);
            let unquoted = unquote(written);
            Value {
                node: *value,
                quoted: unquoted.is_some(),
                text: unquoted.unwrap_or_else(|| written.to_string()),
                role: role(directive, index, *value, arrow, range),
            }
        })
        .collect();
    let Some(last) = values.last() else {
        return;
    };
    let start_byte = anchor.start_byte();
    let end_byte = if range {
        child_end(node, "]").unwrap_or(last.node.end_byte())
    } else {
        last.node.end_byte()
    };
    entries.push(Entry {
        directive,
        node,
        block,
        start_byte,
        end_byte,
        values,
        range,
    });
}

fn role(
    directive: Directive,
    index: usize,
    value: Node<'_>,
    arrow: Option<usize>,
    range: bool,
) -> &'static str {
    match directive {
        Directive::Module => "module_path",
        Directive::Go => "version",
        Directive::Toolchain => "toolchain",
        Directive::Tool => "package_path",
        Directive::Ignore => "path",
        Directive::Godebug if value.kind() == "godebug_key" => "key",
        Directive::Godebug => "value",
        Directive::Retract if range => {
            if index == 0 {
                "low"
            } else {
                "high"
            }
        }
        Directive::Retract => "version",
        Directive::Replace if arrow.is_some_and(|arrow| value.start_byte() > arrow) => {
            if value.kind() == "version" {
                "replacement_version"
            } else {
                "replacement"
            }
        }
        _ if value.kind() == "version" => "version",
        _ => "module_path",
    }
}

fn contains_error(node: Node<'_>, depth: u32) -> bool {
    if node.is_error() {
        return true;
    }
    if !should_visit_tree_depth(depth) {
        return false;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| contains_error(child, child_depth))
}

fn has_child_kind(node: Node<'_>, kind: &str) -> bool {
    child_start(node, kind).is_some()
}

fn child_start(node: Node<'_>, kind: &str) -> Option<usize> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
        .map(|child| child.start_byte())
}

fn child_end(node: Node<'_>, kind: &str) -> Option<usize> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
        .map(|child| child.end_byte())
}

fn node_text<'a>(content: &'a str, node: Node<'_>) -> &'a str {
    content
        .get(node.start_byte()..node.end_byte())
        .unwrap_or("")
}

/// The value of a Go string literal, or `None` for an unquoted token. The
/// grammar lexes a quoted value without spaces as a plain identifier, so this
/// reads the text, not the node kind. An unknown escape is kept as written.
fn unquote(token: &str) -> Option<String> {
    if token.len() < 2 {
        return None;
    }
    if let Some(raw) = token.strip_prefix('`').and_then(|t| t.strip_suffix('`')) {
        return Some(raw.to_string());
    }
    let inner = token.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(escaped @ ('\\' | '"')) => out.push(escaped),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Some(out)
}

/// The `//` lines directly above the line that starts at `start`, markers
/// kept; empty when something other than whitespace precedes `start` on its
/// line.
pub(crate) fn leading_comment_lines(content: &str, start: usize) -> Vec<&str> {
    let line_start = content[..start].rfind('\n').map_or(0, |index| index + 1);
    if !content[line_start..start].trim().is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = content[..line_start]
        .lines()
        .rev()
        .map(str::trim)
        .take_while(|line| line.starts_with("//"))
        .collect();
    lines.reverse();
    lines
}

/// The `//` comment after `end` on the same line.
pub(crate) fn suffix_comment(content: &str, end: usize) -> Option<&str> {
    let rest = content.get(end..)?;
    let line = rest.split('\n').next()?.trim();
    line.starts_with("//").then_some(line)
}

pub(crate) fn doc_comment(content: &str, entry: &Entry<'_>) -> Option<String> {
    let lines = leading_comment_lines(content, entry.start_byte);
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The directive comment text as `modfile` reads it: the leading and suffix
/// comments of the line, else those of its block, each without `//` and
/// trimmed, joined with newlines.
pub(crate) fn directive_comment(content: &str, entry: &Entry<'_>) -> String {
    let mut lines = leading_comment_lines(content, entry.start_byte);
    lines.extend(suffix_comment(content, entry.end_byte));
    if lines.is_empty()
        && let Some(block) = entry.block
    {
        lines = leading_comment_lines(content, block.start_byte());
        if let Some(open) = child_end(block, "(") {
            lines.extend(suffix_comment(content, open));
        }
    }
    lines
        .iter()
        .map(|line| line.trim_start_matches("//").trim())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `modfile.isIndirect`: the suffix comment is `// indirect` or starts with
/// `// indirect;`.
pub(crate) fn is_indirect(content: &str, entry: &Entry<'_>) -> bool {
    let Some(comment) = suffix_comment(content, entry.end_byte) else {
        return false;
    };
    let fields: Vec<&str> = comment
        .trim_start_matches("//")
        .split_whitespace()
        .collect();
    matches!(fields.as_slice(), ["indirect"]) || (fields.len() > 1 && fields[0] == "indirect;")
}

/// The `Deprecated:` paragraph of the module directive comment, as
/// `modfile.parseDeprecation` reads it.
pub(crate) fn deprecation(content: &str, entry: &Entry<'_>) -> Option<String> {
    directive_comment(content, entry)
        .split("\n\n")
        .find_map(|paragraph| paragraph.strip_prefix("Deprecated:"))
        .map(|message| message.trim_start_matches(' ').to_string())
}

/// True when `comment` starts its line and belongs to the leading comment
/// block of the directive line that follows it.
pub(crate) fn comment_documents_following_directive(content: &str, comment: Node<'_>) -> bool {
    let start = comment.start_byte();
    let line_start = content[..start].rfind('\n').map_or(0, |index| index + 1);
    if !content[line_start..start].trim().is_empty() {
        return false;
    }
    let mut offset = content[start..]
        .find('\n')
        .map_or(content.len(), |index| start + index + 1);
    while offset < content.len() {
        let line_end = content[offset..]
            .find('\n')
            .map_or(content.len(), |index| offset + index);
        let line = &content[offset..line_end];
        let trimmed = line.trim_start();
        if trimmed.trim_end().is_empty() {
            return false;
        }
        if !trimmed.starts_with("//") {
            return starts_directive_line(comment, offset + (line.len() - trimmed.len()));
        }
        offset = line_end + 1;
    }
    false
}

fn starts_directive_line(anchor: Node<'_>, byte: usize) -> bool {
    let mut root = anchor;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    let mut node = root.descendant_for_byte_range(byte, byte);
    while let Some(current) = node.filter(|current| current.start_byte() == byte) {
        let kind = current.kind();
        if (kind.ends_with("_directive") || kind.ends_with("_spec") || kind == "tool")
            && !contains_error(current, 0)
        {
            return true;
        }
        node = current.parent();
    }
    false
}

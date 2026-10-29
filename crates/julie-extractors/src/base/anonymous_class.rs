//! Shared naming for anonymous classes and object expressions.
//!
//! An anonymous class (`new Task() { … }`, `object : Task { … }`, an unbound
//! `class { … }` expression) gets a `class` row named `anonymous_class_L<line>`
//! after the line it starts on, with metadata `type: anonymous_class`, so its
//! members have a class owner instead of the enclosing function.

use super::{BaseExtractor, Symbol, SymbolKind};
use tree_sitter::Node;

pub(crate) const ANONYMOUS_CLASS_TYPE: &str = "anonymous_class";

/// The generated name of the anonymous class that `node` starts.
pub(crate) fn anonymous_class_name(node: &Node) -> String {
    format!("anonymous_class_L{}", node.start_position().row + 1)
}

/// True for a row written for an anonymous class.
pub(crate) fn is_anonymous_class(symbol: &Symbol) -> bool {
    symbol.kind == SymbolKind::Class
        && symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("type"))
            .and_then(serde_json::Value::as_str)
            == Some(ANONYMOUS_CLASS_TYPE)
}

/// The source text of `node` before `body`, with each run of whitespace
/// reduced to one space: `new Task()` for `new Task() { … }`.
pub(crate) fn anonymous_class_head(base: &BaseExtractor, node: &Node, body: &Node) -> String {
    let start = node.start_byte();
    let end = body.start_byte().max(start);
    base.content
        .get(start..end)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

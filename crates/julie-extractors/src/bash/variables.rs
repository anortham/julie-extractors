//! Variable and declaration extraction for Bash
//!
//! Plain assignments, declaration commands (`declare`, `local`, `export`,
//! `readonly`, `typeset`) with their flags, and bare declared names.

use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node;

/// Matches ALL_CAPS environment variable names
static ENV_VAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z_][A-Z0-9_]*$").unwrap());

/// The attributes a declaration command gives its names.
pub(crate) struct DeclarationFlags {
    pub(crate) keyword: String,
    pub(crate) readonly: bool,
    pub(crate) exported: bool,
    /// `-g` declares a global from inside a function.
    pub(crate) global: bool,
    /// `-f`/`-F` name functions and `-p` prints, so no variable is declared.
    pub(crate) declares_no_variable: bool,
}

pub(crate) fn declaration_flags(content: &str, node: Node<'_>) -> DeclarationFlags {
    let text = |node: Node<'_>| content.get(node.byte_range()).unwrap_or_default();
    let keyword = node.child(0).map(text).unwrap_or_default().to_string();
    let mut letters = String::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "word"
            && let Some(flags) = text(child).strip_prefix('-')
        {
            letters.push_str(flags);
        }
    }
    let has = |letter: char| letters.contains(letter);
    let exported = if keyword == "export" {
        !has('n') && !has('p')
    } else {
        has('x')
    };
    DeclarationFlags {
        readonly: keyword == "readonly" || has('r'),
        exported,
        global: has('g'),
        declares_no_variable: has('f') || has('F') || has('p') || (keyword == "export" && has('n')),
        keyword,
    }
}

/// The names a declaration command declares, from assignments and bare names.
pub(crate) fn declared_names<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter_map(|child| match child.kind() {
            "variable_name" => Some(child),
            "variable_assignment" => child
                .child_by_field_name("name")
                .filter(|name| name.kind() == "variable_name"),
            _ => None,
        })
        .collect()
}

impl super::BashExtractor {
    /// Extract a variable assignment (VAR=value). A command prefix such as
    /// `IFS= read` sets the environment of one command, and `map[k]=v` writes
    /// an element, so neither declares a variable.
    pub(super) fn extract_variable(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        if node
            .parent()
            .is_some_and(|parent| parent.kind() == "command")
        {
            return None;
        }
        let name_node = node
            .child_by_field_name("name")
            .filter(|name| name.kind() == "variable_name")?;
        let name = self.base.get_node_text(&name_node);

        let options = SymbolOptions {
            signature: self.extract_variable_signature(node),
            visibility: Some(Visibility::Private),
            parent_id: parent_id.map(|s| s.to_string()),
            doc_comment: self.base.find_doc_comment(&node),
            ..Default::default()
        };

        let symbol_kind = if ENV_VAR_RE.is_match(&name) {
            SymbolKind::Constant
        } else {
            SymbolKind::Variable
        };
        Some(self.base.create_symbol(&node, name, symbol_kind, options))
    }

    /// Push one symbol per name a declaration command declares, and return
    /// each assignment value with the symbol that owns it. A bare
    /// `export NAME` or `readonly NAME` after `NAME=...` in the same scope
    /// updates that symbol instead of adding a second one.
    pub(super) fn extract_declarations<'a>(
        &mut self,
        node: Node<'a>,
        parent_id: Option<&str>,
        symbols: &mut Vec<Symbol>,
    ) -> Vec<(Node<'a>, Option<String>)> {
        let flags = declaration_flags(&self.base.content, node);
        let parent_id = parent_id.filter(|_| !flags.global);
        let mut values = Vec::new();
        let mut cursor = node.walk();
        let children: Vec<Node<'a>> = node.named_children(&mut cursor).collect();
        let Some(first_declared) = children
            .iter()
            .find(|child| matches!(child.kind(), "variable_name" | "variable_assignment"))
        else {
            return values;
        };
        let prefix = self
            .base
            .content
            .get(node.start_byte()..first_declared.start_byte())
            .map(str::trim)
            .unwrap_or(&flags.keyword)
            .to_string();
        let doc_comment = self.base.find_doc_comment(&node);
        let kind = if flags.readonly {
            SymbolKind::Constant
        } else {
            SymbolKind::Variable
        };
        let visibility = if flags.exported {
            Visibility::Public
        } else {
            Visibility::Private
        };

        for child in children {
            let value = child.child_by_field_name("value");
            let name_node = match child.kind() {
                "variable_name" => Some(child),
                "variable_assignment" => child
                    .child_by_field_name("name")
                    .filter(|name| name.kind() == "variable_name"),
                _ => None,
            };
            let Some(name_node) = name_node.filter(|_| !flags.declares_no_variable) else {
                values.extend(value.map(|value| (value, None)));
                continue;
            };
            let name = self.base.get_node_text(&name_node);
            if child.kind() == "variable_name"
                && let Some(existing) = symbols.iter_mut().rev().find(|symbol| {
                    symbol.name == name
                        && symbol.parent_id.as_deref() == parent_id
                        && matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Constant)
                })
            {
                if flags.exported {
                    existing.visibility = Some(Visibility::Public);
                }
                if flags.readonly {
                    existing.kind = SymbolKind::Constant;
                }
                continue;
            }
            let symbol = self.base.create_symbol(
                &child,
                name,
                kind.clone(),
                SymbolOptions {
                    signature: Some(format!("{prefix} {}", self.base.get_node_text(&child))),
                    visibility: Some(visibility.clone()),
                    parent_id: parent_id.map(str::to_string),
                    doc_comment: doc_comment.clone(),
                    ..Default::default()
                },
            );
            values.extend(value.map(|value| (value, Some(symbol.id.clone()))));
            symbols.push(symbol);
        }
        values
    }
}

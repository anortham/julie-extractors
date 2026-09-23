//! SQL doc comments.
//!
//! A declaration takes the comment block directly above it, or above the
//! `statement` that wraps it. A comment that trails the previous item on its
//! own line belongs to that item, so it never documents the next one. A
//! comment on the same line after the declaration documents it when there is
//! no comment above. Container comments never reach columns or parameters.
//!
//! `COMMENT ON` statements and MySQL `COMMENT` attributes are catalog docs:
//! they replace the source comment of the object they name.

use crate::base::extractor::select_doc_comment_block;
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::sql::helpers::sql_string_literal_text;
use crate::sql::references::object_reference_parts;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

pub(crate) fn find_sql_doc_comment(base: &BaseExtractor, node: Node) -> Option<String> {
    let anchor = node
        .parent()
        .filter(|parent| parent.kind() == "statement")
        .unwrap_or(node);
    leading_comment(base, anchor)
        .or_else(|| trailing_comment(base, anchor))
        .or_else(|| (node.kind() == "cte").then(|| inner_cte_comment(base, node))?)
}

fn leading_comment(base: &BaseExtractor, anchor: Node) -> Option<String> {
    let mut comments = Vec::new();
    let mut current = anchor.prev_named_sibling();
    while let Some(comment) = current.filter(is_comment) {
        let previous = comment.prev_named_sibling();
        if previous.is_some_and(|previous| {
            !is_comment(&previous) && previous.end_position().row == comment.start_position().row
        }) {
            break;
        }
        comments.push(base.get_node_text(&comment));
        current = previous;
    }
    select_doc_comment_block(&base.language, &comments)
}

fn trailing_comment(base: &BaseExtractor, anchor: Node) -> Option<String> {
    let comment = anchor
        .next_named_sibling()
        .filter(is_comment)
        .filter(|comment| comment.start_position().row == anchor.end_position().row)?;
    select_doc_comment_block(&base.language, &[base.get_node_text(&comment)])
}

fn inner_cte_comment(base: &BaseExtractor, cte: Node) -> Option<String> {
    let mut cursor = cte.walk();
    let mut comments: Vec<String> = cte
        .children(&mut cursor)
        .filter(is_comment)
        .map(|comment| base.get_node_text(&comment))
        .collect();
    comments.reverse();
    select_doc_comment_block(&base.language, &comments)
}

fn is_comment(node: &Node) -> bool {
    matches!(node.kind(), "comment" | "marginalia")
}

/// Apply `COMMENT ON` statements and MySQL `COMMENT` attributes.
pub(super) fn apply_catalog_comments(base: &BaseExtractor, root: Node, symbols: &mut [Symbol]) {
    let mut comments = Vec::new();
    collect_catalog_comments(base, root, &mut comments, 0);
    for comment in comments {
        let target = match &comment.target {
            CatalogTarget::Object(parts) => find_object(symbols, parts),
            CatalogTarget::Column { table, column } => find_column(symbols, table, column),
            CatalogTarget::Symbol(start_byte, kind) => symbols
                .iter()
                .position(|symbol| symbol.start_byte == *start_byte && symbol.kind == *kind),
        };
        if let Some(index) = target {
            symbols[index].doc_comment = Some(comment.text);
        }
    }
}

enum CatalogTarget {
    Object(Vec<String>),
    Column { table: Vec<String>, column: String },
    Symbol(u32, SymbolKind),
}

struct CatalogComment {
    target: CatalogTarget,
    text: String,
}

fn collect_catalog_comments(
    base: &BaseExtractor,
    node: Node,
    comments: &mut Vec<CatalogComment>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "comment_statement" => comments.extend(comment_statement(base, node)),
        "column_definition" => {
            if let Some(text) = column_comment_attribute(base, node) {
                comments.push(CatalogComment {
                    target: CatalogTarget::Symbol(node.start_byte() as u32, SymbolKind::Field),
                    text,
                });
            }
        }
        "table_option" => {
            if let Some(comment) = table_comment_option(base, node) {
                comments.push(comment);
            }
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_catalog_comments(base, child, comments, child_depth);
    }
}

fn comment_statement(base: &BaseExtractor, node: Node) -> Option<CatalogComment> {
    let reference = base.find_child_by_type(&node, "object_reference")?;
    let text =
        sql_string_literal_text(&base.get_node_text(&base.find_child_by_type(&node, "literal")?))?;
    let target = if base.find_child_by_type(&node, "keyword_column").is_some() {
        column_target(base, reference)?
    } else {
        CatalogTarget::Object(object_reference_parts(base, reference))
    };
    Some(CatalogComment { target, text })
}

/// `COMMENT ON COLUMN s.t.c` nests the table reference inside the column
/// reference.
fn column_target(base: &BaseExtractor, reference: Node) -> Option<CatalogTarget> {
    if let Some(table) = base.find_child_by_type(&reference, "object_reference") {
        let column = reference.child_by_field_name("name")?;
        return Some(CatalogTarget::Column {
            table: object_reference_parts(base, table),
            column: crate::sql::helpers::normalize_sql_identifier(&base.get_node_text(&column)),
        });
    }
    let mut parts = object_reference_parts(base, reference);
    let column = parts.pop()?;
    Some(CatalogTarget::Column {
        table: parts,
        column,
    })
}

fn column_comment_attribute(base: &BaseExtractor, column: Node) -> Option<String> {
    let keyword = base.find_child_by_type(&column, "keyword_comment")?;
    let literal = keyword
        .next_named_sibling()
        .filter(|node| node.kind() == "literal")?;
    sql_string_literal_text(&base.get_node_text(&literal))
}

fn table_comment_option(base: &BaseExtractor, option: Node) -> Option<CatalogComment> {
    let name = option.child_by_field_name("name")?;
    if !base.get_node_text(&name).eq_ignore_ascii_case("comment") {
        return None;
    }
    let text = base.get_node_text(&option);
    let (_, value) = text.split_once('=')?;
    let table = option
        .parent()
        .filter(|parent| parent.kind() == "create_table")?;
    Some(CatalogComment {
        target: CatalogTarget::Symbol(table.start_byte() as u32, SymbolKind::Class),
        text: sql_string_literal_text(value.trim())?,
    })
}

fn find_object(symbols: &[Symbol], parts: &[String]) -> Option<usize> {
    let (name, schema) = parts.split_last()?;
    symbols.iter().position(|symbol| {
        symbol.parent_id.is_none()
            && symbol.name == *name
            && matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Function
            )
            && match (schema.last(), symbol_schema(symbol)) {
                (Some(wanted), Some(declared)) => wanted == declared,
                _ => true,
            }
    })
}

fn find_column(symbols: &[Symbol], table: &[String], column: &str) -> Option<usize> {
    let table_id = symbols[find_object(symbols, table)?].id.clone();
    symbols.iter().position(|symbol| {
        symbol.kind == SymbolKind::Field
            && symbol.name == column
            && symbol.parent_id.as_deref() == Some(table_id.as_str())
    })
}

fn symbol_schema(symbol: &Symbol) -> Option<&String> {
    match symbol.metadata.as_ref()?.get("schema")? {
        serde_json::Value::String(schema) => Some(schema),
        _ => None,
    }
}

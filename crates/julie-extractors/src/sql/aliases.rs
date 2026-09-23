//! Table aliases in query scopes.
//!
//! A column reference names its table through an alias (`o.total`), the
//! table name itself (`orders.total`), or nothing when the query reads one
//! table. Each `statement`, `subquery`, and view query is a scope; an inner
//! scope sees the aliases of the scopes around it.

use crate::base::BaseExtractor;
use crate::sql::helpers::normalize_sql_identifier;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// The table a column reference reads, from its qualifier or from the only
/// table its query reads.
pub(super) fn column_table(base: &BaseExtractor, field: Node) -> Option<String> {
    match field
        .named_children(&mut field.walk())
        .find(|child| child.kind() == "object_reference")
    {
        Some(qualifier) => {
            let name = qualifier
                .child_by_field_name("name")
                .map(|name| normalize_sql_identifier(&base.get_node_text(&name)))?;
            let mut scope = enclosing_scope(field);
            while let Some(current) = scope {
                let bindings = scope_tables(base, current);
                let matching = |explicit: bool| {
                    bindings.iter().find(|binding| {
                        binding.explicit == explicit && binding.alias.eq_ignore_ascii_case(&name)
                    })
                };
                if let Some(binding) = matching(true).or_else(|| matching(false)) {
                    return binding.table.clone();
                }
                scope = current.parent().and_then(enclosing_scope);
            }
            None
        }
        None => {
            let tables = scope_tables(base, enclosing_scope(field)?);
            let first = tables.first()?.table.clone()?;
            tables
                .iter()
                .all(|binding| binding.table.as_ref() == Some(&first))
                .then_some(first)
        }
    }
}

/// The table an `INSERT` or `MERGE` writes: the statement's first table
/// reference.
pub(super) fn write_target_table(base: &BaseExtractor, column_list: Node) -> Option<String> {
    let mut current = column_list.parent();
    while let Some(node) = current {
        if matches!(node.kind(), "insert" | "statement") {
            let reference = node
                .named_children(&mut node.walk())
                .find(|child| child.kind() == "object_reference")?;
            let name = reference.child_by_field_name("name")?;
            return Some(normalize_sql_identifier(&base.get_node_text(&name)));
        }
        current = node.parent();
    }
    None
}

/// `table` is `None` for a derived table (`FROM (SELECT ...) sub`) or a
/// table variable. An `explicit` alias outranks a bare table name, so
/// `UPDATE a ... FROM accounts a` binds `a` to `accounts`.
struct TableBinding {
    alias: String,
    table: Option<String>,
    explicit: bool,
}

fn enclosing_scope(node: Node) -> Option<Node> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if is_scope(candidate.kind()) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn is_scope(kind: &str) -> bool {
    matches!(kind, "statement" | "subquery" | "create_query")
}

fn scope_tables(base: &BaseExtractor, scope: Node) -> Vec<TableBinding> {
    let mut bindings = Vec::new();
    collect_bindings(base, scope, scope, &mut bindings, 0);
    bindings
}

fn collect_bindings(
    base: &BaseExtractor,
    scope: Node,
    node: Node,
    bindings: &mut Vec<TableBinding>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.id() != scope.id() && is_scope(node.kind()) {
        return;
    }
    let binding = match node.kind() {
        "relation" => match node
            .named_children(&mut node.walk())
            .find(|child| child.kind() == "object_reference")
        {
            Some(reference) => binding_for(base, reference, node.child_by_field_name("alias")),
            None => Some(TableBinding {
                alias: node
                    .child_by_field_name("alias")
                    .map(|alias| normalize_sql_identifier(&base.get_node_text(&alias)))
                    .unwrap_or_default(),
                table: None,
                explicit: true,
            }),
        },
        "object_reference"
            if node.parent().is_some_and(|parent| {
                matches!(parent.kind(), "from" | "insert" | "create_index")
            }) || is_merge_table(node) =>
        {
            let alias = node
                .next_named_sibling()
                .and_then(|sibling| match sibling.kind() {
                    "keyword_as" => sibling.next_named_sibling(),
                    _ => Some(sibling),
                })
                .filter(|sibling| sibling.kind() == "identifier");
            binding_for(base, node, alias)
        }
        _ => None,
    };
    if let Some(binding) = binding {
        bindings.push(binding);
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        collect_bindings(base, scope, child, bindings, child_depth);
    }
}

fn is_merge_table(reference: Node) -> bool {
    reference.parent().is_some_and(|statement| {
        statement.kind() == "statement"
            && statement
                .child(0)
                .is_some_and(|first| first.kind() == "keyword_merge")
    })
}

fn binding_for(base: &BaseExtractor, reference: Node, alias: Option<Node>) -> Option<TableBinding> {
    let table =
        normalize_sql_identifier(&base.get_node_text(&reference.child_by_field_name("name")?));
    let explicit = alias.is_some();
    let alias = alias
        .map(|alias| normalize_sql_identifier(&base.get_node_text(&alias)))
        .unwrap_or_else(|| table.clone());
    Some(TableBinding {
        alias,
        table: (!table.starts_with('@')).then_some(table),
        explicit,
    })
}

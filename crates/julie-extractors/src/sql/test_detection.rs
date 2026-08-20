use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::sql::helpers::normalize_sql_identifier;
use std::collections::HashSet;
use std::collections::VecDeque;
use tree_sitter::{Node, Tree};

#[derive(Debug, Default)]
pub(super) struct PgTapContext {
    runner_seen: bool,
    runner_schemas: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PgTapRoutineRole {
    Case,
    Lifecycle,
}

impl PgTapContext {
    pub(super) fn from_tree(base: &BaseExtractor, tree: &Tree) -> Self {
        let mut pending = VecDeque::from([tree.root_node()]);
        let mut context = Self::default();
        while let Some(node) = pending.pop_front() {
            if is_pgtap_runner(base, node) {
                context.runner_seen = true;
                if let Some(schema) = pgtap_runner_schema(base, node) {
                    context.runner_schemas.insert(schema);
                }
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        context
    }

    fn runner_seen(&self) -> bool {
        self.runner_seen
    }

    fn runner_schemas(&self) -> &HashSet<String> {
        &self.runner_schemas
    }
}

pub(super) fn mark_pgtap_schema_containers(context: &PgTapContext, symbols: &mut [Symbol]) {
    if context.runner_schemas().is_empty() {
        return;
    }

    for symbol in symbols {
        if symbol.kind != SymbolKind::Namespace
            || !symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("isSchema"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        {
            continue;
        }

        let schema_name = normalize_sql_identifier(&symbol.name).to_ascii_lowercase();
        if context.runner_schemas().contains(&schema_name) {
            symbol
                .metadata
                .get_or_insert_with(Default::default)
                .insert("test_container".to_string(), serde_json::Value::Bool(true));
        }
    }
}

pub(super) fn classify_routine(
    base: &BaseExtractor,
    node: Node,
    name: &str,
    context: &PgTapContext,
) -> Option<PgTapRoutineRole> {
    if !context.runner_seen() || !returns_setof_text(base, node) {
        return None;
    }

    let name = normalize_sql_identifier(name).to_ascii_lowercase();
    if name.starts_with("test") {
        return Some(PgTapRoutineRole::Case);
    }
    if ["startup", "setup", "teardown", "shutdown"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return Some(PgTapRoutineRole::Lifecycle);
    }
    None
}

fn is_pgtap_runner(base: &BaseExtractor, node: Node) -> bool {
    matches!(
        call_name(base, node).as_deref(),
        Some("runtests" | "do_tap")
    )
}

fn pgtap_runner_schema(base: &BaseExtractor, node: Node) -> Option<String> {
    if !is_pgtap_runner(base, node) {
        return None;
    }

    let mut cursor = node.walk();
    let argument = node
        .named_children(&mut cursor)
        .find(|child| child.kind() != "object_reference")?;
    let raw = base.get_node_text(&argument);
    let raw = raw.trim();
    let unquoted = raw
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .unwrap_or(raw);
    let schema = normalize_sql_identifier(unquoted);
    (!schema.is_empty()).then(|| schema.to_ascii_lowercase())
}

fn call_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let name_node = if node.kind() == "invocation" {
        let mut cursor = node.walk();
        let object_ref = node
            .named_children(&mut cursor)
            .find(|child| child.kind() == "object_reference")?;
        object_ref
            .child_by_field_name("name")
            .or_else(|| object_ref.named_child(0))?
    } else if node.kind() == "identifier"
        && node
            .next_named_sibling()
            .is_some_and(|sibling| sibling.kind() == "function_arguments")
    {
        node
    } else {
        return None;
    };

    Some(normalize_sql_identifier(&base.get_node_text(&name_node)).to_ascii_lowercase())
}

fn returns_setof_text(base: &BaseExtractor, node: Node) -> bool {
    let mut saw_setof = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "keyword_setof" {
            saw_setof = true;
            continue;
        }
        if !saw_setof {
            continue;
        }
        if child.kind() == "function_body" || child.kind() == "function_language" {
            return false;
        }
        if child.kind() == "keyword_text"
            || normalize_sql_identifier(&base.get_node_text(&child)).eq_ignore_ascii_case("text")
        {
            return true;
        }
    }
    false
}

use crate::base::{BaseExtractor, Symbol, SymbolKind, TestRole};
use crate::sql::helpers::normalize_sql_identifier;
use crate::test_detection::apply_test_role;
use std::collections::HashSet;
use std::collections::VecDeque;
use tree_sitter::{Node, Tree};

/// Test framework evidence found anywhere in the file: pgTAP runner calls
/// and tSQLt test classes (`EXEC tSQLt.NewTestClass 'OrderTests'`).
#[derive(Debug, Default)]
pub(super) struct PgTapContext {
    runner_seen: bool,
    runner_schemas: HashSet<String>,
    tsqlt_classes: HashSet<String>,
}

/// pgTAP name prefixes that select a fixture hook, and the half it runs on.
const PGTAP_LIFECYCLE_PREFIXES: [(&str, TestRole); 4] = [
    ("startup", TestRole::FixtureSetup),
    ("setup", TestRole::FixtureSetup),
    ("teardown", TestRole::FixtureTeardown),
    ("shutdown", TestRole::FixtureTeardown),
];

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
            if let Some(class) = tsqlt_new_test_class(base, node) {
                context.tsqlt_classes.insert(class);
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

    fn is_test_schema(&self, schema: &str) -> bool {
        self.runner_schemas.contains(schema) || self.tsqlt_classes.contains(schema)
    }
}

pub(super) fn mark_pgtap_schema_containers(context: &PgTapContext, symbols: &mut [Symbol]) {
    if context.runner_schemas().is_empty() && context.tsqlt_classes.is_empty() {
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
        if context.is_test_schema(&schema_name) {
            apply_test_role(
                symbol.metadata.get_or_insert_with(Default::default),
                TestRole::TestContainer,
            );
        }
    }
}

pub(super) fn classify_routine(
    base: &BaseExtractor,
    node: Node,
    name: &str,
    context: &PgTapContext,
) -> Option<TestRole> {
    if let Some(role) = classify_tsqlt_procedure(base, node, name, context) {
        return Some(role);
    }
    if !context.runner_seen() || !returns_setof_text(base, node) {
        return None;
    }

    let name = normalize_sql_identifier(name).to_ascii_lowercase();
    if name.starts_with("test") {
        return Some(TestRole::TestCase);
    }
    PGTAP_LIFECYCLE_PREFIXES
        .iter()
        .find(|(prefix, _)| name.starts_with(prefix))
        .map(|(_, role)| *role)
}

/// tSQLt runs every procedure of a test class whose name starts with `test`,
/// and runs the class's `SetUp` procedure before each test.
fn classify_tsqlt_procedure(
    base: &BaseExtractor,
    node: Node,
    name: &str,
    context: &PgTapContext,
) -> Option<TestRole> {
    if !node.kind().ends_with("procedure") {
        return None;
    }
    let reference = base.find_child_by_type(&node, "object_reference")?;
    let schema = reference.child_by_field_name("schema")?;
    let schema = normalize_sql_identifier(&base.get_node_text(&schema)).to_ascii_lowercase();
    if !context.tsqlt_classes.contains(&schema) {
        return None;
    }
    let name = normalize_sql_identifier(name).to_ascii_lowercase();
    if name.starts_with("test") {
        Some(TestRole::TestCase)
    } else if name == "setup" {
        Some(TestRole::FixtureSetup)
    } else {
        None
    }
}

fn tsqlt_new_test_class(base: &BaseExtractor, node: Node) -> Option<String> {
    if node.kind() != "execute_statement" {
        return None;
    }
    let reference = base.find_child_by_type(&node, "object_reference")?;
    let parts = super::references::object_reference_parts(base, reference);
    let [schema, name] = parts.as_slice() else {
        return None;
    };
    if !schema.eq_ignore_ascii_case("tsqlt") || !name.eq_ignore_ascii_case("newtestclass") {
        return None;
    }
    let argument = node.child_by_field_name("parameter")?;
    let class = crate::sql::helpers::sql_string_literal_text(&base.get_node_text(&argument))?;
    Some(normalize_sql_identifier(class.trim()).to_ascii_lowercase())
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
    let raw = raw.split("::").next().unwrap_or_default().trim();
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

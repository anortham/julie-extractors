use crate::base::TestRole;
use crate::test_detection::apply_test_role;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

pub(crate) struct TomlTestContext {
    trycmd_dotted_case: bool,
    trycmd_table_case: bool,
    nextest_marker: bool,
}

impl TomlTestContext {
    /// A trycmd case names its binary (`bin.name`, or `name` in `[bin]`) and
    /// at least one case field: `args`, `status`, `stdout`, or `stderr`. The
    /// streams are optional because trycmd also reads `.stdout`/`.stderr`
    /// sidecar files. Nextest config is marked by `nextest-version` or
    /// `experimental`, or by its canonical path `.config/nextest.toml`.
    pub(crate) fn from_tree(tree: &Tree, source: &str, file_path: &str) -> Self {
        let mut scan = Scan::default();
        scan.visit(tree.root_node(), source, None, 0);
        Self {
            trycmd_dotted_case: scan.root_bin_name && scan.root_case_field,
            trycmd_table_case: scan.table_bin_name
                && (scan.table_case_field || scan.root_case_field),
            nextest_marker: scan.nextest_marker || is_nextest_config_path(file_path),
        }
    }

    pub(crate) fn pair_role(&self, table_name: Option<&str>, key_name: &str) -> Option<TestRole> {
        if self.trycmd_dotted_case && table_name.is_none() && key_name == "bin.name" {
            return Some(TestRole::TestCase);
        }
        None
    }

    pub(crate) fn table_role(&self, table_name: &str) -> Option<TestRole> {
        if self.trycmd_table_case && table_name == "bin" {
            return Some(TestRole::TestCase);
        }
        if self.nextest_marker && is_named_table(table_name, "test-groups.") {
            return Some(TestRole::TestContainer);
        }
        if self.nextest_marker && is_named_table(table_name, "scripts.setup.") {
            return Some(TestRole::FixtureSetup);
        }
        None
    }

    pub(crate) fn metadata(role: Option<TestRole>) -> Option<HashMap<String, Value>> {
        let mut metadata = HashMap::new();
        apply_test_role(&mut metadata, role?);
        Some(metadata)
    }
}

#[derive(Default)]
struct Scan {
    root_bin_name: bool,
    root_case_field: bool,
    table_bin_name: bool,
    table_case_field: bool,
    nextest_marker: bool,
}

impl Scan {
    fn visit(&mut self, node: Node<'_>, source: &str, current_table: Option<&str>, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let mut table_name = current_table.map(str::to_owned);
        if matches!(node.kind(), "table" | "table_array_element")
            && let Some(name) = super::header_name(node, source)
        {
            if current_table.is_none() && name == "experimental" {
                self.nextest_marker = true;
            }
            table_name = Some(name);
        }

        if node.kind() == "pair"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() != "inline_table")
            && let Some(key) = super::dependencies::pair_key_parts(node, source)
            && let Some(value) = super::pair_value(node)
        {
            let key: Vec<&str> = key.iter().map(String::as_str).collect();
            let table = table_name.as_deref();
            if table.is_none() && matches!(key.as_slice(), ["nextest-version"] | ["experimental"]) {
                self.nextest_marker = true;
            }
            match (table, key.as_slice()) {
                (None, ["bin", "name"]) if value.kind() == "string" => self.root_bin_name = true,
                (None, [field, ..]) if is_case_field(field) => self.root_case_field = true,
                (Some("bin"), ["name"]) if value.kind() == "string" => {
                    self.table_bin_name = true;
                }
                (Some("bin"), [field, ..]) if is_case_field(field) => {
                    self.table_case_field = true;
                }
                _ => {}
            }
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit(child, source, table_name.as_deref(), child_depth);
        }
    }
}

fn is_case_field(field: &str) -> bool {
    matches!(field, "args" | "status" | "stdout" | "stderr")
}

fn is_nextest_config_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    normalized == ".config/nextest.toml" || normalized.ends_with("/.config/nextest.toml")
}

fn is_named_table(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|suffix| !suffix.is_empty() && !suffix.contains('.'))
}

pub(crate) fn role_metadata(role: Option<TestRole>) -> Option<HashMap<String, Value>> {
    TomlTestContext::metadata(role)
}

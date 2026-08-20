use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TomlTestRole {
    Case,
    Container,
    Lifecycle,
}

pub(crate) struct TomlTestContext {
    trycmd_dotted_case: bool,
    trycmd_table_case: bool,
    nextest_marker: bool,
}

impl TomlTestContext {
    pub(crate) fn from_tree(tree: &Tree, source: &str) -> Self {
        let mut scan = Scan::default();
        scan.visit(tree.root_node(), source, None);
        Self {
            trycmd_dotted_case: scan.root_bin_name
                && scan.root_status
                && scan.root_stdout
                && scan.root_stderr,
            trycmd_table_case: scan.table_bin_name
                && scan.table_status
                && scan.table_stdout
                && scan.table_stderr,
            nextest_marker: scan.nextest_marker,
        }
    }

    pub(crate) fn pair_role(
        &self,
        table_name: Option<&str>,
        key_name: &str,
    ) -> Option<TomlTestRole> {
        if self.trycmd_dotted_case && table_name.is_none() && key_name == "bin.name" {
            return Some(TomlTestRole::Case);
        }
        None
    }

    pub(crate) fn table_role(&self, table_name: &str) -> Option<TomlTestRole> {
        if self.trycmd_table_case && table_name == "bin" {
            return Some(TomlTestRole::Case);
        }
        if self.nextest_marker && is_named_table(table_name, "test-groups.") {
            return Some(TomlTestRole::Container);
        }
        if self.nextest_marker && is_named_table(table_name, "scripts.setup.") {
            return Some(TomlTestRole::Lifecycle);
        }
        None
    }

    pub(crate) fn metadata(role: Option<TomlTestRole>) -> Option<HashMap<String, Value>> {
        let role = role?;
        let mut metadata = HashMap::new();
        match role {
            TomlTestRole::Case => {
                metadata.insert("is_test".to_string(), Value::Bool(true));
            }
            TomlTestRole::Container => {
                metadata.insert("test_container".to_string(), Value::Bool(true));
            }
            TomlTestRole::Lifecycle => {
                metadata.insert("is_test".to_string(), Value::Bool(true));
                metadata.insert("test_lifecycle".to_string(), Value::Bool(true));
            }
        }
        Some(metadata)
    }
}

#[derive(Default)]
struct Scan {
    root_bin_name: bool,
    root_status: bool,
    root_stdout: bool,
    root_stderr: bool,
    table_bin_name: bool,
    table_status: bool,
    table_stdout: bool,
    table_stderr: bool,
    nextest_marker: bool,
}

impl Scan {
    fn visit(&mut self, node: Node<'_>, source: &str, current_table: Option<&str>) {
        let mut table_name = current_table.map(str::to_owned);
        if matches!(node.kind(), "table" | "table_array_element")
            && let Some(name) = key_text(node, source)
        {
            if current_table.is_none() && name == "experimental" {
                self.nextest_marker = true;
            }
            table_name = Some(name);
        }

        if node.kind() == "pair"
            && let Some((key_name, value_kind)) = pair_data(node, source)
        {
            let table = table_name.as_deref();
            if table.is_none() && matches!(key_name.as_str(), "nextest-version" | "experimental") {
                self.nextest_marker = true;
            }
            match (table, key_name.as_str(), value_kind) {
                (None, "bin.name", ValueKind::String) => self.root_bin_name = true,
                (None, "status", ValueKind::Integer) => self.root_status = true,
                (None, "stdout", ValueKind::String) => self.root_stdout = true,
                (None, "stderr", ValueKind::String) => self.root_stderr = true,
                (Some("bin"), "name", ValueKind::String) => self.table_bin_name = true,
                (Some("bin"), "status", ValueKind::Integer) => self.table_status = true,
                (Some("bin"), "stdout", ValueKind::String) => self.table_stdout = true,
                (Some("bin"), "stderr", ValueKind::String) => self.table_stderr = true,
                _ => {}
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit(child, source, table_name.as_deref());
        }
    }
}

#[derive(Clone, Copy)]
enum ValueKind {
    String,
    Integer,
    Other,
}

fn pair_data(node: Node<'_>, source: &str) -> Option<(String, ValueKind)> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    if children.len() < 3 {
        return None;
    }
    let key_name = key_text(children[0], source)?;
    let value_kind = match children.last()?.kind() {
        "string" => ValueKind::String,
        "integer" => ValueKind::Integer,
        _ => ValueKind::Other,
    };
    Some((key_name, value_kind))
}

fn key_text(node: Node<'_>, source: &str) -> Option<String> {
    key_text_at_depth(node, source, 0)
}

fn key_text_at_depth(node: Node<'_>, source: &str, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    if matches!(node.kind(), "bare_key" | "quoted_key" | "dotted_key") {
        return node
            .utf8_text(source.as_bytes())
            .ok()
            .map(|text| text.trim_matches('"').trim_matches('\'').to_string());
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(name) = key_text_at_depth(child, source, child_depth) {
            return Some(name);
        }
    }
    None
}

fn is_named_table(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|suffix| !suffix.is_empty() && !suffix.contains('.'))
}

pub(crate) fn role_metadata(role: Option<TomlTestRole>) -> Option<HashMap<String, Value>> {
    TomlTestContext::metadata(role)
}

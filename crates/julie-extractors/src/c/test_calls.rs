//! C Criterion call-style test extraction (Miller bridge test-roles).
//!
//! Criterion declares tests with a macro that the C grammar parses as a *call*,
//! not a function definition:
//!
//! ```c
//! Test(math, addition) {
//!     cr_assert(2 + 2 == 4);
//! }
//! ```
//!
//! tree-sitter-c parses `Test(math, addition)` as a `call_expression` (function
//! field = identifier `Test`, arguments = an `argument_list` of two bare
//! `identifier`s — the suite and the test name) followed by a DETACHED
//! `compound_statement` block (the block is a sibling of the call, not a child).
//! Unlike the JS/Dart string-named DSLs, Criterion's name is built from the two
//! identifier arguments joined `suite.name`. Only that grammar walking is
//! C-local; classification and symbol construction delegate to the shared
//! `crate::test_calls` core so the captured `is_test` metadata is identical across
//! languages and the downstream `classify_symbols_by_role` pass treats Criterion
//! like every other call-style framework.
//!
//! Optional trailing arguments are ignored when building test names.

use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::test_calls::{
    TestCallCategory, TestCallVocab, build_test_call_symbol, classify_call_exact,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

const CRITERION_VOCAB: TestCallVocab = TestCallVocab {
    test: &["Test"],
    container: &["TestSuite"],
    lifecycle: &[],
};

/// Materialize a Criterion test or suite call as a test-role symbol.
/// Returns `None` for any call that is not a recognized Criterion test macro
/// (e.g. `cr_assert(...)`, `printf(...)`), so the caller can blindly invoke it for
/// every `call_expression` and only Criterion tests become symbols.
pub fn extract_c_test_call(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if node.kind() != "call_expression" {
        return None;
    }

    let function_node = node.child_by_field_name("function")?;
    let full_callee = base.get_node_text(&function_node);
    let category = classify_call_exact(&full_callee, &CRITERION_VOCAB)?;

    let args_node = node.child_by_field_name("arguments")?;
    let name = match category {
        TestCallCategory::Test => {
            let mut cursor = args_node.walk();
            let identifier_args: Vec<String> = args_node
                .children(&mut cursor)
                .filter(|c| c.kind() == "identifier")
                .take(2)
                .map(|c| base.get_node_text(&c))
                .collect();
            if identifier_args.len() < 2 {
                return None;
            }
            identifier_args.join(".")
        }
        TestCallCategory::Container => first_identifier(base, args_node)?,
        TestCallCategory::Lifecycle => return None,
    };

    Some(build_test_call_symbol(
        base,
        node,
        &full_callee,
        name,
        category,
        parent_id,
    ))
}

pub fn apply_criterion_lifecycle_metadata(
    base: &BaseExtractor,
    root: Node,
    symbols: &mut [Symbol],
) {
    let mut lifecycle_names = HashSet::new();
    collect_lifecycle_names(base, root, &mut lifecycle_names, 0);
    if lifecycle_names.is_empty() {
        return;
    }

    for symbol in symbols {
        if !matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            || !lifecycle_names.contains(&symbol.name)
        {
            continue;
        }
        let metadata = symbol.metadata.get_or_insert_with(Default::default);
        metadata.insert("is_test".to_string(), serde_json::json!(true));
        metadata.insert("test_lifecycle".to_string(), serde_json::json!(true));
    }
}

fn collect_lifecycle_names(
    base: &BaseExtractor,
    node: Node,
    names: &mut HashSet<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "call_expression"
        && let Some(function) = node.child_by_field_name("function")
        && matches!(base.get_node_text(&function).as_str(), "Test" | "TestSuite")
        && let Some(arguments) = node.child_by_field_name("arguments")
    {
        collect_designated_lifecycle_names(base, arguments, names);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_lifecycle_names(base, child, names, child_depth);
    }
}

fn collect_designated_lifecycle_names(
    base: &BaseExtractor,
    node: Node,
    names: &mut HashSet<String>,
) {
    let source = base.get_node_text(&node);
    for marker in [".init", ".fini"] {
        let mut offset = 0;
        while let Some(relative) = source[offset..].find(marker) {
            let marker_end = offset + relative + marker.len();
            let after_marker = source[marker_end..].trim_start();
            let Some(after_equals) = after_marker.strip_prefix('=') else {
                offset = marker_end;
                continue;
            };
            let hook = after_equals.trim_start();
            let hook_len = hook
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .map(char::len_utf8)
                .sum::<usize>();
            if hook_len > 0 {
                names.insert(hook[..hook_len].to_string());
            }
            offset = marker_end;
        }
    }
}

fn first_identifier(base: &BaseExtractor, node: Node) -> Option<String> {
    first_identifier_at_depth(base, node, 0)
}

fn first_identifier_at_depth(base: &BaseExtractor, node: Node, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == "identifier" {
        return Some(base.get_node_text(&node));
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(identifier) = first_identifier_at_depth(base, child, child_depth) {
            return Some(identifier);
        }
    }
    None
}

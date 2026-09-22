//! Lua busted call-style test extraction.
//!
//! Like JS/TS (Jest/Vitest) and Dart (`package:test`), busted tests are call
//! expressions, not named function declarations:
//!
//! ```lua
//! describe("math", function()
//!   before_each(function() end)
//!   it("adds", function() assert.equal(2, 1 + 1) end)
//! end)
//! ```
//!
//! The grammar shape is `function_call` with a `name` field (the callee — a bare
//! `identifier` for DSL calls; a `dot_index_expression` such as `assert.equal`
//! for method calls, which classify to `None`) and an `arguments` field whose
//! first `string` child carries the description. Only the grammar walking is
//! Lua-local; classification and symbol construction delegate to the shared
//! `crate::test_calls` core so the captured `is_test` / `test_container` /
//! `test_lifecycle` metadata is byte-identical to the JS/TS and Dart paths and
//! the downstream `classify_symbols_by_role` pass treats them identically.

use crate::base::{BaseExtractor, Symbol, SymbolKind, TestRole};
use crate::test_calls::{
    TestCallCategory, TestCallVocab, build_test_call_symbol, classify_call_exact,
};
use crate::test_detection::{apply_test_role, is_test_path};
use tree_sitter::Node;

/// busted vocabulary. `it`/`test`/`spec`/`pending` are cases,
/// `describe`/`context`/`insulate`/`expose` are containers, and the setup and
/// teardown hooks (including the `strict_` forms and a test's `finally`
/// cleanup) are lifecycle fixtures.
pub(crate) const LUA_VOCAB: TestCallVocab = TestCallVocab {
    test: &["it", "test", "spec", "pending"],
    container: &["describe", "context", "insulate", "expose"],
    lifecycle: &[
        "before_each",
        "after_each",
        "setup",
        "teardown",
        "lazy_setup",
        "lazy_teardown",
        "strict_setup",
        "strict_teardown",
        "finally",
    ],
};

/// busted DSL calls count only in a test file (a test path or a `_spec.lua`
/// file) or in a file that loads busted; elsewhere `setup(...)` or `it(...)` is
/// an ordinary call.
fn busted_dsl_is_active(base: &BaseExtractor) -> bool {
    let file_name = base
        .file_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&base.file_path);
    is_test_path(&base.file_path)
        || file_name.ends_with("_spec.lua")
        || base.content.contains("require(\"busted")
        || base.content.contains("require \"busted")
        || base.content.contains("require('busted")
}

/// Mark luaunit test tables (`TestCalc = {}` whose name starts with `test`,
/// any case) at the top level of a test file as test containers.
pub(super) fn mark_luaunit_test_containers(base: &BaseExtractor, symbols: &mut [Symbol]) {
    if !is_test_path(&base.file_path) {
        return;
    }
    for symbol in symbols.iter_mut().filter(|symbol| {
        symbol.parent_id.is_none()
            && matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Class)
            && symbol.name.to_ascii_lowercase().starts_with("test")
            && symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("dataType"))
                .and_then(|data_type| data_type.as_str())
                == Some("table")
    }) {
        apply_test_role(
            symbol.metadata.get_or_insert_with(Default::default),
            TestRole::TestContainer,
        );
    }
}

/// Materialize a busted `function_call` as a test/container/lifecycle symbol.
/// Returns `None` for any call that is not a recognized busted DSL call (e.g.
/// `assert.equal(...)`, `require(...)`, `print(...)`), so the caller can invoke
/// it for every `function_call` and only DSL calls become symbols.
pub(super) fn extract_lua_test_call(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if node.kind() != "function_call" {
        return None;
    }

    let callee_node = node.child_by_field_name("name")?;
    let full_callee = base.get_node_text(&callee_node);
    // Exact match only (#66): a dot-index method call (`it.register(...)`, name
    // field = `dot_index_expression`) never equals a dotless busted DSL name, so
    // the exact-matcher rejects it without the JS-only leading-segment split.
    let category = classify_call_exact(&full_callee, &LUA_VOCAB)?;
    if !busted_dsl_is_active(base) {
        return None;
    }

    let name = match category {
        // Lifecycle calls take no description string; use the callee's base name.
        TestCallCategory::Lifecycle => full_callee
            .split('.')
            .next()
            .unwrap_or(&full_callee)
            .to_string(),
        // it/describe/context take the description as the first string argument.
        _ => {
            let args_node = node.child_by_field_name("arguments")?;
            let mut cursor = args_node.walk();
            let first_string = args_node
                .children(&mut cursor)
                .find(|c| c.kind() == "string")?;
            base.decode_string_literal(&first_string)?
        }
    };

    Some(build_test_call_symbol(
        base,
        &node,
        &full_callee,
        name,
        category,
        parent_id,
    ))
}

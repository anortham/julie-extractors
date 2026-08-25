//! Go Ginkgo/Gomega call-style test extraction (Miller bridge test-roles, Wave-3).
//!
//! Ginkgo declares tests as call expressions (`call_expression` nodes in the
//! Go grammar), not named function declarations:
//!
//! ```go
//! var _ = Describe("math", func() {
//!     Context("addition", func() {
//!         BeforeEach(func() { })
//!         AfterEach(func() { })
//!         It("should add two numbers", func() {
//!             Expect(1 + 1).To(Equal(2))
//!         })
//!     })
//! })
//! ```
//!
//! Grammar shape (confirmed via live AST probe against tree-sitter-go):
//! - Node kind: `call_expression`
//! - Callee: `function` **field** → `identifier` node (text = `"Describe"`, `"It"`, …)
//! - Description string: `arguments` **field** → `argument_list` → first named child
//!   whose kind is `interpreted_string_literal` → decoded via `base.decode_string_literal`.
//! - Lifecycle calls (`BeforeEach`, `AfterEach`, `BeforeSuite`, `AfterSuite`,
//!   `JustBeforeEach`, `JustAfterEach`, `DeferCleanup`) take only a closure — no
//!   description string. The callee name is used as the symbol name.
//!
//! Focused/pending variants (`FDescribe`, `FIt`, `XDescribe`, `XIt`, `PDescribe`,
//! `PIt`, …) are included so Ginkgo focus/skip markers are still materialised.
//!
//! The standard Go `testing.T`-based idiom (`func TestXxx(t *testing.T)` + `_test.go`
//! path detection) was handled in task #48 via `classify_symbols_by_role`. This
//! adapter is purely additive.

use crate::base::{BaseExtractor, Symbol};
use crate::test_calls::{
    TestCallCategory, TestCallVocab, build_test_call_symbol, classify_call_exact,
};
use crate::test_detection::is_go_test_file;
use tree_sitter::Node;

/// The Ginkgo module path, as it is spelled in an import for every major
/// version (`.../ginkgo`, `.../ginkgo/v2`).
const GINKGO_MODULE_PATH: &str = "github.com/onsi/ginkgo";

/// A Ginkgo DSL call that became a symbol, with the category the caller needs
/// to scope leaf nodes to their container.
pub(super) struct GinkgoTestCall {
    pub(super) symbol: Symbol,
    pub(super) category: TestCallCategory,
}

/// Whether Ginkgo's DSL words may be read as tests in this file.
///
/// `Describe`, `Context`, `When`, and `It` are ordinary Go identifiers that
/// production code defines and calls for its own reasons, so a call only counts
/// when `go test` compiles the file or the file imports Ginkgo itself.
pub(super) fn file_enables_ginkgo(base: &BaseExtractor, root: Node) -> bool {
    is_go_test_file(&base.file_path) || imports_ginkgo(base, root)
}

/// Go puts every import in an `import_declaration` directly under the source
/// file, holding either one `import_spec` or an `import_spec_list` of them, so
/// two levels of plain iteration reach every import path.
fn imports_ginkgo(base: &BaseExtractor, root: Node) -> bool {
    let mut root_cursor = root.walk();
    root.children(&mut root_cursor)
        .filter(|child| child.kind() == "import_declaration")
        .any(|declaration| {
            let mut declaration_cursor = declaration.walk();
            declaration
                .children(&mut declaration_cursor)
                .any(|child| declares_ginkgo_import(base, child))
        })
}

fn declares_ginkgo_import(base: &BaseExtractor, node: Node) -> bool {
    match node.kind() {
        "import_spec" => base.get_node_text(&node).contains(GINKGO_MODULE_PATH),
        "import_spec_list" => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .filter(|spec| spec.kind() == "import_spec")
                .any(|spec| base.get_node_text(&spec).contains(GINKGO_MODULE_PATH))
        }
        _ => false,
    }
}

/// Ginkgo v2 vocabulary (ecosystem-knowledge; verified against
/// <https://pkg.go.dev/github.com/onsi/ginkgo/v2> container/leaf/setup nodes).
///
/// - Container: `Describe` + focused/excluded/table variants, `Context`, `When`
/// - Test: `It`, `Specify` + focused/excluded variants
/// - Lifecycle: `BeforeEach`, `AfterEach`, `BeforeSuite`, `AfterSuite`,
///   `JustBeforeEach`, `JustAfterEach`, `DeferCleanup`
const GINKGO_VOCAB: TestCallVocab = TestCallVocab {
    test: &[
        "It", "FIt", "XIt", "PIt", "Specify", "FSpecify", "XSpecify", "PSpecify", "Entry",
        "FEntry", "XEntry", "PEntry",
    ],
    container: &[
        "Describe",
        "FDescribe",
        "XDescribe",
        "PDescribe",
        "Context",
        "FContext",
        "XContext",
        "PContext",
        "When",
        "FWhen",
        "XWhen",
        "PWhen",
        "DescribeTable",
        "FDescribeTable",
        "XDescribeTable",
        "PDescribeTable",
    ],
    lifecycle: &[
        "BeforeEach",
        "AfterEach",
        "BeforeSuite",
        "AfterSuite",
        "JustBeforeEach",
        "JustAfterEach",
        "DeferCleanup",
    ],
};

/// Materialize a Ginkgo `call_expression` as a test/container/lifecycle symbol.
/// Returns `None` for any call that is not a recognised Ginkgo DSL call (e.g.
/// `fmt.Println(...)`, `http.Get(...)`), so the caller can invoke this for every
/// `call_expression` node and only DSL calls become symbols.
pub(super) fn extract_ginkgo_test_call(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<GinkgoTestCall> {
    if node.kind() != "call_expression" {
        return None;
    }

    // Callee lives in the `function` field of the `call_expression`.
    // Ginkgo DSL calls are bare identifiers (`Describe`, `It`, …), not selectors.
    let function_node = node.child_by_field_name("function")?;
    if function_node.kind() != "identifier" {
        return None; // skip selector_expression (e.g. `g.Describe(…)`)
    }
    let full_callee = base.get_node_text(&function_node);
    // Exact match only (#66): the `function.kind() != "identifier"` guard above
    // already rejects `selector_expression` callees — use the exact-matcher
    // uniformly so the JS-only `.`-split never applies.
    let category = classify_call_exact(&full_callee, &GINKGO_VOCAB)?;

    let name = match category {
        // Lifecycle: no description string; use the callee name.
        TestCallCategory::Lifecycle => full_callee.to_string(),
        // Describe/Context/It/Specify — first string argument is the description.
        _ => {
            let args_node = node.child_by_field_name("arguments")?; // argument_list
            let mut cursor = args_node.walk();
            let str_arg = args_node
                .named_children(&mut cursor)
                .find(|c| c.kind().contains("string_literal"))?;
            base.decode_string_literal(&str_arg)?
        }
    };

    Some(GinkgoTestCall {
        symbol: build_test_call_symbol(base, &node, &full_callee, name, category, parent_id),
        category,
    })
}

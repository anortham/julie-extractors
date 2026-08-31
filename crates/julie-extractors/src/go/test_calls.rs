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
//! module adds literal `testing.T.Run` subtest symbols to those declarations.

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
pub(crate) const GINKGO_VOCAB: TestCallVocab = TestCallVocab {
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

pub(super) fn extract_standard_subtest_call(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
    enclosing_test: bool,
) -> Option<Symbol> {
    if !enclosing_test || node.kind() != "call_expression" {
        return None;
    }

    let function_node = node.child_by_field_name("function")?;
    if function_node.kind() != "selector_expression" {
        return None;
    }
    let operand = function_node.child_by_field_name("operand")?;
    let field = function_node.child_by_field_name("field")?;
    if operand.kind() != "identifier"
        || field.kind() != "field_identifier"
        || base.get_node_text(&field) != "Run"
    {
        return None;
    }

    let receiver_name = base.get_node_text(&operand);
    if !active_testing_t_receiver(base, node, &receiver_name) {
        return None;
    }

    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let mut named_arguments = arguments.named_children(&mut cursor);
    let name_node = named_arguments.next()?;
    if !name_node.kind().contains("string_literal") {
        return None;
    }
    let callback = named_arguments.next()?;
    if callback.kind() != "func_literal" || !is_testing_t_callback(base, callback) {
        return None;
    }

    let name = base.decode_string_literal(&name_node)?;
    Some(build_test_call_symbol(
        base,
        &node,
        "Run",
        name,
        TestCallCategory::Test,
        parent_id,
    ))
}

fn is_testing_t_callback(base: &BaseExtractor, callback: Node) -> bool {
    if callback.child_by_field_name("result").is_some() {
        return false;
    }

    let Some(parameters) = callback.child_by_field_name("parameters") else {
        return false;
    };
    let mut cursor = parameters.walk();
    let parameters: Vec<Node> = parameters.named_children(&mut cursor).collect();
    let [parameter] = parameters.as_slice() else {
        return false;
    };
    if parameter.kind() != "parameter_declaration" {
        return false;
    }

    let Some(type_node) = parameter.child_by_field_name("type") else {
        return false;
    };
    let mut parameter_cursor = parameter.walk();
    let name_count = parameter
        .named_children(&mut parameter_cursor)
        .filter(|child| child.kind() == "identifier")
        .count();
    name_count <= 1 && base.get_node_text(&type_node) == "*testing.T"
}

fn active_testing_t_receiver(base: &BaseExtractor, call_node: Node, receiver_name: &str) -> bool {
    let mut ancestor = call_node.parent();
    while let Some(node) = ancestor {
        if scope_introduces_local_binding(base, node, call_node, receiver_name) {
            return false;
        }
        if node.kind() == "block" && local_binding_before_call(base, node, call_node, receiver_name)
        {
            return false;
        }
        if matches!(
            node.kind(),
            "function_declaration" | "method_declaration" | "func_literal"
        ) {
            if named_result_binds_name(base, node, receiver_name) {
                return false;
            }
            if let Some(is_testing_t) = testing_t_parameter(base, node, receiver_name) {
                return is_testing_t;
            }
        }
        ancestor = node.parent();
    }
    false
}

fn scope_introduces_local_binding(
    base: &BaseExtractor,
    node: Node,
    call_node: Node,
    receiver_name: &str,
) -> bool {
    match node.kind() {
        "if_statement" | "expression_switch_statement" | "for_clause" => node
            .child_by_field_name("initializer")
            .filter(|initializer| initializer.end_byte() <= call_node.start_byte())
            .is_some_and(|initializer| statement_binds_name(base, initializer, receiver_name)),
        "for_statement" => for_statement_introduces_binding(base, node, call_node, receiver_name),
        "select_statement" => select_receive_binds_name(base, node, call_node, receiver_name),
        "type_switch_statement" => {
            let initializer_binding = node
                .child_by_field_name("initializer")
                .filter(|initializer| initializer.end_byte() <= call_node.start_byte())
                .is_some_and(|initializer| statement_binds_name(base, initializer, receiver_name));
            let alias_binding = node.child_by_field_name("alias").is_some_and(|alias| {
                switch_case_contains_call(node, call_node)
                    && declaration_names_include(base, alias, receiver_name)
            });
            initializer_binding || alias_binding
        }
        _ => false,
    }
}

fn for_statement_introduces_binding(
    base: &BaseExtractor,
    for_statement: Node,
    call_node: Node,
    receiver_name: &str,
) -> bool {
    let mut cursor = for_statement.walk();
    for clause in for_statement.named_children(&mut cursor) {
        match clause.kind() {
            "for_clause" => {
                if clause
                    .child_by_field_name("initializer")
                    .filter(|initializer| initializer.end_byte() <= call_node.start_byte())
                    .is_some_and(|initializer| {
                        statement_binds_name(base, initializer, receiver_name)
                    })
                {
                    return true;
                }
            }
            "range_clause"
                if call_is_in_for_body(for_statement, call_node)
                    && range_clause_binds_name(base, clause, receiver_name) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn call_is_in_for_body(for_statement: Node, call_node: Node) -> bool {
    let Some(body) = for_statement.child_by_field_name("body") else {
        return false;
    };
    call_node.start_byte() >= body.start_byte() && call_node.end_byte() <= body.end_byte()
}

fn range_clause_binds_name(base: &BaseExtractor, clause: Node, receiver_name: &str) -> bool {
    if !base.get_node_text(&clause).contains(":=") {
        return false;
    }
    let Some(left) = clause.child_by_field_name("left") else {
        return false;
    };
    declaration_names_include(base, left, receiver_name)
}

fn switch_case_contains_call(switch: Node, call_node: Node) -> bool {
    let mut ancestor = call_node.parent();
    while let Some(node) = ancestor {
        if node.id() == switch.id() {
            return false;
        }
        if matches!(node.kind(), "type_case" | "default_case") {
            return true;
        }
        ancestor = node.parent();
    }
    false
}

fn select_receive_binds_name(
    base: &BaseExtractor,
    select: Node,
    call_node: Node,
    receiver_name: &str,
) -> bool {
    let mut ancestor = call_node.parent();
    while let Some(node) = ancestor {
        if node.id() == select.id() {
            return false;
        }
        if node.kind() == "communication_case" {
            let mut case_cursor = node.walk();
            let Some(body) = node
                .named_children(&mut case_cursor)
                .find(|child| child.kind() == "statement_list")
            else {
                return false;
            };
            if call_node.start_byte() < body.start_byte() || call_node.end_byte() > body.end_byte()
            {
                return false;
            }

            let Some(communication) = node.child_by_field_name("communication") else {
                return false;
            };
            if communication.kind() != "receive_statement"
                || !base.get_node_text(&communication).contains(":=")
            {
                return false;
            }
            let Some(left) = communication.child_by_field_name("left") else {
                return false;
            };
            return declaration_names_include(base, left, receiver_name);
        }
        ancestor = node.parent();
    }
    false
}

fn local_binding_before_call(
    base: &BaseExtractor,
    block: Node,
    call_node: Node,
    receiver_name: &str,
) -> bool {
    let mut block_cursor = block.walk();
    let Some(statement_list) = block
        .named_children(&mut block_cursor)
        .find(|child| child.kind() == "statement_list")
    else {
        return false;
    };

    let mut statement_cursor = statement_list.walk();
    statement_list
        .named_children(&mut statement_cursor)
        .take_while(|statement| statement.end_byte() <= call_node.start_byte())
        .any(|statement| statement_binds_name(base, statement, receiver_name))
}

fn statement_binds_name(base: &BaseExtractor, statement: Node, receiver_name: &str) -> bool {
    match statement.kind() {
        "var_declaration" => var_declaration_binds_name(base, statement, receiver_name),
        "short_var_declaration" => short_var_declaration_binds_name(base, statement, receiver_name),
        _ => false,
    }
}

fn var_declaration_binds_name(
    base: &BaseExtractor,
    declaration: Node,
    receiver_name: &str,
) -> bool {
    let mut declaration_cursor = declaration.walk();
    declaration
        .named_children(&mut declaration_cursor)
        .any(|child| match child.kind() {
            "var_spec" => declaration_names_include(base, child, receiver_name),
            "var_spec_list" => {
                let mut list_cursor = child.walk();
                child
                    .named_children(&mut list_cursor)
                    .filter(|spec| spec.kind() == "var_spec")
                    .any(|spec| declaration_names_include(base, spec, receiver_name))
            }
            _ => false,
        })
}

fn short_var_declaration_binds_name(
    base: &BaseExtractor,
    declaration: Node,
    receiver_name: &str,
) -> bool {
    let Some(left) = declaration.child_by_field_name("left") else {
        return false;
    };
    declaration_names_include(base, left, receiver_name)
}

fn declaration_names_include(base: &BaseExtractor, node: Node, receiver_name: &str) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| child.kind() == "identifier" && base.get_node_text(&child) == receiver_name)
}

fn named_result_binds_name(base: &BaseExtractor, function_node: Node, receiver_name: &str) -> bool {
    let Some(result) = function_node.child_by_field_name("result") else {
        return false;
    };
    if result.kind() != "parameter_list" {
        return false;
    }
    let mut result_cursor = result.walk();
    result
        .named_children(&mut result_cursor)
        .filter(|parameter| parameter.kind() == "parameter_declaration")
        .any(|parameter| declaration_names_include(base, parameter, receiver_name))
}

fn testing_t_parameter(
    base: &BaseExtractor,
    function_node: Node,
    receiver_name: &str,
) -> Option<bool> {
    let parameters = function_node.child_by_field_name("parameters")?;
    let mut cursor = parameters.walk();
    for parameter in parameters.named_children(&mut cursor) {
        let type_node = parameter.child_by_field_name("type")?;
        let mut parameter_cursor = parameter.walk();
        let receiver_bound = parameter
            .named_children(&mut parameter_cursor)
            .any(|child| {
                child.kind() == "identifier" && base.get_node_text(&child) == receiver_name
            });
        if receiver_bound {
            return Some(base.get_node_text(&type_node) == "*testing.T");
        }
    }
    None
}

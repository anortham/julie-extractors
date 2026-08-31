//! Go Ginkgo call-style test detection (Miller bridge test-roles, Wave-3).
//!
//! Ginkgo declares tests as call expressions, not named function declarations:
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
//!   that is an `interpreted_string_literal` → decoded via `base.decode_string_literal`.
//! - Lifecycle calls (`BeforeEach`, `AfterEach`, `BeforeSuite`, `AfterSuite`,
//!   `JustBeforeEach`, `JustAfterEach`) take only a closure argument — no description
//!   string. The callee name is used as the symbol name.

use crate::base::Symbol;
use crate::go::GoExtractor;
use std::path::PathBuf;

fn symbols_in(file_path: &str, code: &str) -> Vec<Symbol> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .expect("load Go grammar");
    let tree = parser.parse(code, None).expect("parse Go");
    let mut ext = GoExtractor::new(
        "go".to_string(),
        file_path.to_string(),
        code.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    ext.extract_symbols(&tree)
}

fn symbols(code: &str) -> Vec<Symbol> {
    symbols_in("math_test.go", code)
}

fn meta_bool(s: &Symbol, key: &str) -> bool {
    s.metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn meta_string<'a>(s: &'a Symbol, key: &str) -> Option<&'a str> {
    s.metadata.as_ref()?.get(key)?.as_str()
}

// ── Wave-3 tests ─────────────────────────────────────────────────────────

#[test]
fn ginkgo_describe_context_it_lifecycle_emit_test_role_metadata() {
    let code = r#"package math_test

var _ = Describe("math module", func() {
    Context("addition", func() {
        BeforeEach(func() {})
        AfterEach(func() {})
        It("should add two numbers", func() {
            Expect(1 + 1).To(Equal(2))
        })
        Specify("one plus one equals two", func() {
            Expect(1 + 1).To(Equal(2))
        })
    })
    BeforeSuite(func() {})
    AfterSuite(func() {})
})
"#;
    let syms = symbols(code);

    let desc = syms
        .iter()
        .find(|s| s.name == "math module")
        .unwrap_or_else(|| panic!("expected Describe container, got: {syms:?}"));
    assert!(
        meta_bool(desc, "test_container"),
        "Describe → test_container"
    );
    assert!(!meta_bool(desc, "is_test"), "container is not a test case");

    let ctx = syms
        .iter()
        .find(|s| s.name == "addition")
        .unwrap_or_else(|| panic!("expected Context container, got: {syms:?}"));
    assert!(meta_bool(ctx, "test_container"), "Context → test_container");

    let it = syms
        .iter()
        .find(|s| s.name == "should add two numbers")
        .unwrap_or_else(|| panic!("expected It test case, got: {syms:?}"));
    assert!(meta_bool(it, "is_test"), "It → is_test");
    assert!(!meta_bool(it, "test_container"), "test is not a container");

    let specify = syms
        .iter()
        .find(|s| s.name == "one plus one equals two")
        .unwrap_or_else(|| panic!("expected Specify test case, got: {syms:?}"));
    assert!(meta_bool(specify, "is_test"), "Specify → is_test");

    for lifecycle_name in ["BeforeEach", "AfterEach", "BeforeSuite", "AfterSuite"] {
        let lc = syms
            .iter()
            .find(|s| s.name == lifecycle_name)
            .unwrap_or_else(|| panic!("expected {lifecycle_name} lifecycle symbol, got: {syms:?}"));
        assert!(
            meta_bool(lc, "is_test"),
            "{lifecycle_name} → is_test (lifecycle)",
        );
        assert!(
            meta_bool(lc, "test_lifecycle"),
            "{lifecycle_name} → test_lifecycle",
        );
    }
}

#[test]
fn non_ginkgo_go_calls_do_not_become_test_symbols() {
    // fmt.Println, http.Get, and ordinary function calls must not carry test-role
    // metadata.
    let code = r#"package main

import "fmt"

func main() {
    fmt.Println("hello world")
    result := someFunc("argument")
    _ = result
}
"#;
    let syms = symbols(code);
    assert_eq!(
        syms.iter()
            .filter(|s| meta_bool(s, "is_test") || meta_bool(s, "test_container"))
            .count(),
        0,
        "non-Ginkgo calls must not carry test-role metadata: {syms:?}"
    );
}

#[test]
fn selector_calls_with_vocab_names_are_not_test_symbols() {
    // Locks in the `function_node.kind() != "identifier"` selector guard.
    // `req.Context()`, `s.It("…")`, `s.Describe("…", func(){})` all have
    // TRAILING names that are in the Ginkgo vocab — but they are
    // `selector_expression` callees, not bare identifiers. Without the guard
    // they would silently materialise as test containers/cases across ordinary
    // Go web and struct code. If the guard is ever removed, this test fails.
    let code = r#"package main

import "net/http"

func handler(req *http.Request) {
    ctx := req.Context()
    _ = ctx
}

type suite struct{}

func (s suite) run() {
    s.It("not a ginkgo test")
    s.Describe("nope", func() {})
    s.BeforeEach(func() {})
}
"#;
    let syms = symbols(code);
    assert_eq!(
        syms.iter()
            .filter(|s| meta_bool(s, "is_test") || meta_bool(s, "test_container"))
            .count(),
        0,
        "selector calls with vocab names must NOT produce test-role metadata: {syms:?}"
    );
}

#[test]
fn bare_ginkgo_vocabulary_in_production_go_is_not_a_test() {
    let code = r#"package scheduler

func Describe(name string, run func()) {}

func It(name string, run func()) {}

func Register() {
    Describe("job queue", func() {
        It("drains", func() {})
    })
}
"#;
    let syms = symbols_in("scheduler/scheduler.go", code);
    assert_eq!(
        syms.iter()
            .filter(|s| meta_bool(s, "is_test") || meta_bool(s, "test_container"))
            .count(),
        0,
        "production Go without a Ginkgo import must publish no test roles: {syms:?}"
    );
}

#[test]
fn ginkgo_import_enables_detection_outside_a_test_file() {
    let code = r#"package shared

import . "github.com/onsi/ginkgo/v2"

var _ = Describe("shared behaviour", func() {
    It("holds", func() {})
})
"#;
    let syms = symbols_in("shared/behaviour.go", code);
    let container = syms
        .iter()
        .find(|s| s.name == "shared behaviour")
        .unwrap_or_else(|| panic!("expected Describe container, got: {syms:?}"));
    assert!(meta_bool(container, "test_container"));
    let case = syms
        .iter()
        .find(|s| s.name == "holds")
        .unwrap_or_else(|| panic!("expected It case, got: {syms:?}"));
    assert!(meta_bool(case, "is_test"));
}

#[test]
fn ginkgo_leaf_without_a_container_ancestor_loses_its_role() {
    let code = r#"package math_test

func sharedExpectations() {
    It("has no container", func() {})
    BeforeEach(func() {})
}

var _ = Describe("math", func() {
    It("has a container", func() {})
})
"#;
    let syms = symbols(code);
    let orphan = syms
        .iter()
        .find(|s| s.name == "has no container")
        .unwrap_or_else(|| panic!("expected orphan It symbol, got: {syms:?}"));
    assert!(!meta_bool(orphan, "is_test"));
    let orphan_hook = syms
        .iter()
        .find(|s| s.name == "BeforeEach")
        .unwrap_or_else(|| panic!("expected orphan BeforeEach symbol, got: {syms:?}"));
    assert!(!meta_bool(orphan_hook, "is_test"));
    let scoped = syms
        .iter()
        .find(|s| s.name == "has a container")
        .unwrap_or_else(|| panic!("expected scoped It symbol, got: {syms:?}"));
    assert!(meta_bool(scoped, "is_test"));
}

#[test]
fn testify_suite_struct_is_a_test_container() {
    let code = r#"package math_test

import (
    "sync"

    "github.com/stretchr/testify/suite"
)

type CalculatorSuite struct {
    suite.Suite
}

type recordingClock struct {
    sync.Mutex
}
"#;
    let syms = symbols(code);
    let suite_struct = syms
        .iter()
        .find(|s| s.name == "CalculatorSuite")
        .unwrap_or_else(|| panic!("expected suite struct, got: {syms:?}"));
    assert!(meta_bool(suite_struct, "test_container"));
    let control = syms
        .iter()
        .find(|s| s.name == "recordingClock")
        .unwrap_or_else(|| panic!("expected control struct, got: {syms:?}"));
    assert!(!meta_bool(control, "test_container"));
}

#[test]
fn a_suite_struct_outside_a_test_file_is_not_a_container() {
    let code = r#"package harness

import "github.com/stretchr/testify/suite"

type CalculatorSuite struct {
    suite.Suite
}
"#;
    let syms = symbols_in("harness/harness.go", code);
    let suite_struct = syms
        .iter()
        .find(|s| s.name == "CalculatorSuite")
        .unwrap_or_else(|| panic!("expected suite struct, got: {syms:?}"));
    assert!(!meta_bool(suite_struct, "test_container"));
}

#[test]
fn go_test_main_and_benchmark_carry_lifecycle_and_case_roles() {
    let code = r#"package math_test

import "testing"

func TestMain(m *testing.M) {}

func BenchmarkAdds(b *testing.B) {}

func Testable(t *testing.T) {}
"#;
    let syms = symbols(code);
    let main = syms
        .iter()
        .find(|s| s.name == "TestMain")
        .unwrap_or_else(|| panic!("expected TestMain, got: {syms:?}"));
    assert!(meta_bool(main, "test_lifecycle"));
    let benchmark = syms
        .iter()
        .find(|s| s.name == "BenchmarkAdds")
        .unwrap_or_else(|| panic!("expected BenchmarkAdds, got: {syms:?}"));
    assert!(meta_bool(benchmark, "is_test"));
    assert!(!meta_bool(benchmark, "test_lifecycle"));
    let control = syms
        .iter()
        .find(|s| s.name == "Testable")
        .unwrap_or_else(|| panic!("expected Testable, got: {syms:?}"));
    assert!(!meta_bool(control, "is_test"));
}

#[test]
fn literal_t_run_with_arbitrary_parameter_name_emits_child_case() {
    let code = r#"package math_test

import "testing"

func TestAdds(testHandle *testing.T) {
    testHandle.Run("literal child", func(child *testing.T) {})
}
"#;
    let syms = symbols(code);
    let parent = syms
        .iter()
        .find(|s| s.name == "TestAdds")
        .unwrap_or_else(|| panic!("expected TestAdds, got: {syms:?}"));
    let child = syms
        .iter()
        .find(|s| s.name == "literal child")
        .unwrap_or_else(|| panic!("expected literal child, got: {syms:?}"));

    assert_eq!(meta_string(child, "test_role"), Some("test_case"));
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    assert_eq!(syms.iter().filter(|s| s.name == "literal child").count(), 1);
}

#[test]
fn nested_literal_t_runs_preserve_parent_identity() {
    let code = r#"package math_test

import "testing"

func TestAdds(t *testing.T) {
    t.Run("outer", func(outer *testing.T) {
        outer.Run("inner", func(inner *testing.T) {})
    })
}
"#;
    let syms = symbols(code);
    let root = syms
        .iter()
        .find(|s| s.name == "TestAdds")
        .unwrap_or_else(|| panic!("expected TestAdds, got: {syms:?}"));
    let outer = syms
        .iter()
        .find(|s| s.name == "outer")
        .unwrap_or_else(|| panic!("expected outer, got: {syms:?}"));
    let inner = syms
        .iter()
        .find(|s| s.name == "inner")
        .unwrap_or_else(|| panic!("expected inner, got: {syms:?}"));

    assert_eq!(meta_string(outer, "test_role"), Some("test_case"));
    assert_eq!(meta_string(inner, "test_role"), Some("test_case"));
    assert_eq!(outer.parent_id.as_deref(), Some(root.id.as_str()));
    assert_eq!(inner.parent_id.as_deref(), Some(outer.id.as_str()));
}

#[test]
fn dynamic_t_run_names_are_not_test_cases() {
    let code = r#"package math_test

import "testing"

func TestAdds(t *testing.T) {
    name := "dynamic"
    t.Run(name, func(t *testing.T) {})
}
"#;
    let syms = symbols(code);

    assert!(!syms.iter().any(|s| s.name == "dynamic"));
}

#[test]
fn unrelated_run_receiver_types_are_not_test_cases() {
    let code = r#"package math_test

type customT struct{}

func (receiver *customT) Run(name string, callback func()) {}

func helper(receiver *customT) {
    receiver.Run("unrelated receiver", func() {})
}
"#;
    let syms = symbols(code);

    assert!(!syms.iter().any(|s| s.name == "unrelated receiver"));
}

#[test]
fn only_exact_run_member_is_a_standard_subtest() {
    let code = r#"package math_test

import "testing"

func TestAdds(t *testing.T) {
    t.Runner("wrong member", func(t *testing.T) {})
}
"#;
    let syms = symbols(code);

    assert!(!syms.iter().any(|s| s.name == "wrong member"));
}

#[test]
fn non_literal_or_non_function_t_run_arguments_are_not_test_cases() {
    let code = r#"package math_test

import "testing"

func TestAdds(t *testing.T) {
    callback := func(*testing.T) {}
    t.Run("callback value", callback)
}
"#;
    let syms = symbols(code);

    assert!(!syms.iter().any(|s| s.name == "callback value"));
}

#[test]
fn t_run_without_an_enclosing_test_symbol_is_silent() {
    let code = r#"package math_test

import "testing"

var fileScope = t.Run("file scope", func(t *testing.T) {})

func helper(t *testing.T) {
    t.Run("helper scope", func(t *testing.T) {})
}
"#;
    let syms = symbols(code);

    assert!(
        !syms
            .iter()
            .any(|s| { matches!(s.name.as_str(), "file scope" | "helper scope") })
    );
}

#[test]
fn t_run_requires_a_single_testing_t_callback_parameter() {
    let code = r#"package math_test

import "testing"

func TestAdds(t *testing.T) {
    t.Run("no callback parameter", func() {})
    t.Run("wrong callback parameter type", func(value int) {})
    t.Run("multiple callback parameters", func(first *testing.T, second *testing.T) {})
}
"#;
    let syms = symbols(code);

    assert!(!syms.iter().any(|s| {
        matches!(
            s.name.as_str(),
            "no callback parameter"
                | "wrong callback parameter type"
                | "multiple callback parameters"
        )
    }));
}

#[test]
fn local_var_bindings_shadow_the_testing_t_parameter() {
    let code = r#"package math_test

import "testing"

type customT struct{}

func (receiver *customT) Run(name string, callback func()) {}

func TestAdds(t *testing.T) {
    {
        var t = &customT{}
        t.Run("var shadow", func(*testing.T) {})
    }
    {
        t := &customT{}
        t.Run("short shadow", func(*testing.T) {})
    }
}
"#;
    let syms = symbols(code);

    assert!(
        !syms
            .iter()
            .any(|s| { matches!(s.name.as_str(), "var shadow" | "short shadow") })
    );
}

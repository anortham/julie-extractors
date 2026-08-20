//! Python test-role emission: unittest/pytest cases, containers, lifecycle.

use crate::base::{Symbol, SymbolKind};
use crate::python::PythonExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn symbols(code: &str, file: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("load Python grammar");
    let tree = parser.parse(code, None).expect("parse Python");
    let mut ext = PythonExtractor::new(file.to_string(), code.to_string(), &PathBuf::from("/tmp"));
    ext.extract_symbols(&tree)
}

fn role(symbols: &[Symbol], name: &str, key: &str) -> bool {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("expected symbol {name}, got {symbols:?}"))
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

#[test]
fn unittest_setup_emits_test_lifecycle_and_testcase_is_container() {
    let code = r#"
import unittest

class ArithmeticTest(unittest.TestCase):
    def setUp(self):
        self.total = 2

    def tearDown(self):
        pass

    def test_unittest_case(self):
        self.assertEqual(self.total, 2)

    def production_helper(self):
        return self.total

class OrdinaryHelper:
    def helper(self):
        return 1

class MisleadingBase(NotATestCase):
    def helper(self):
        return 1

def test_pytest_case():
    assert True

def calculate_total():
    return 2
"#;
    let syms = symbols(code, "tests/test_arithmetic.py");

    assert!(role(&syms, "ArithmeticTest", "test_container"));
    assert!(!role(&syms, "OrdinaryHelper", "test_container"));
    assert!(!role(&syms, "MisleadingBase", "test_container"));

    assert!(role(&syms, "setUp", "is_test"));
    assert!(role(&syms, "setUp", "test_lifecycle"));
    assert!(role(&syms, "tearDown", "is_test"));
    assert!(role(&syms, "tearDown", "test_lifecycle"));

    assert!(role(&syms, "test_unittest_case", "is_test"));
    assert!(!role(&syms, "test_unittest_case", "test_lifecycle"));
    assert!(role(&syms, "test_pytest_case", "is_test"));
    assert!(!role(&syms, "test_pytest_case", "test_lifecycle"));

    assert!(!role(&syms, "production_helper", "is_test"));
    assert!(!role(&syms, "calculate_total", "is_test"));
    assert!(!role(&syms, "ArithmeticTest", "is_test"));

    assert!(
        syms.iter()
            .any(|s| s.name == "ArithmeticTest" && s.kind == SymbolKind::Class)
    );
}

#[test]
fn fixture_and_patch_helpers_are_not_test_symbols() {
    let code = r#"
import pytest
import unittest

@pytest.fixture
def build_client():
    return object()

@unittest.mock.patch("module.target")
def patch_client():
    return object()
"#;
    let syms = symbols(code, "tests/test_helpers.py");

    assert!(!role(&syms, "build_client", "is_test"));
    assert!(!role(&syms, "patch_client", "is_test"));
}
